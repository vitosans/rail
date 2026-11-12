use super::*;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rail_core::auth::{FileSecretStore, SecretStore};
use rail_core::config::AccountKind;
use rail_core::daemon::SpawnFn;
use rail_core::ops::DaemonInfoRequest;
use rail_core::ops::op::DaemonInfoOp;
use rail_core::store::Store;
use tokio::sync::mpsc;

use crate::tui::test_util::{
    daemon_ctx, gated_daemon_ctx, query, redirect, redirect_when_asked, test_ctx, token_server,
};

const FILE_BACKEND: &str = "[general]\nsecrets = \"file\"\n";
const ACME: &str =
    "name = \"Acme\"\nkind = \"gmail\"\nclient_id = \"acme\"\nclient_secret = \"GOCSPX-acme\"\n";

fn tmp_paths(tmp: &tempfile::TempDir, config: &str) -> Paths {
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), config).unwrap();
    paths
}

async fn run(effect: Effect) -> Vec<Event> {
    let Effect::Spawn(job) = effect else {
        panic!("expected a spawned job, got {effect:?}");
    };
    let (tx, mut results) = mpsc::unbounded_channel();
    job(TaskSink::new(tx, 0)).await;
    let mut events = Vec::new();
    while let Ok((_, TaskResult::Wizard(event))) = results.try_recv() {
        events.push(event);
    }
    events
}

fn account(address: &str, secret: &str) -> AccountConfig {
    AccountConfig {
        name: "me".into(),
        kind: AccountKind::Gmail,
        address: address.into(),
        color: None,
        label: None,
        client_id: "cid".into(),
        client_secret: Some(secret.into()),
        tenant: None,
        signature: None,
        from_name: None,
        ai_send: None,
        history_days: None,
        body_cache_days: None,
        ..Default::default()
    }
}

/// How many operations the daemon still has waiting or saving.
async fn pending_in_daemon(ctx: &EffectContext) -> usize {
    let client = ctx.daemon.client().await.unwrap();
    client
        .call::<DaemonInfoOp>(&DaemonInfoRequest {})
        .await
        .unwrap()
        .pending
}

/// `path` relative to this process's working directory, by climbing to the root.
fn relative_to_cwd(path: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    let mut relative: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
    relative.push(path.strip_prefix("/").unwrap());
    relative
}

#[test]
fn plain_actions_map_to_shell_effects() {
    let ctx = test_ctx();
    assert!(matches!(to_effect(Action::Quit, &ctx), Effect::Quit));
    assert!(matches!(
        to_effect(Action::CancelPending, &ctx),
        Effect::CancelAll
    ));
    assert!(matches!(to_effect(Action::Copy("u".into()), &ctx), Effect::Copy(t) if t == "u"));
}

#[tokio::test]
async fn load_presets_lists_the_daemons_presets_with_a_marker_for_the_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let dir = paths.config_dir.join("presets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("acme.toml"), ACME).unwrap();
    let (ctx, _) = daemon_ctx(&paths, None).await;
    let events = run(to_effect(Action::LoadPresets, &ctx)).await;
    let [Event::PresetsLoaded(entries)] = events.as_slice() else {
        panic!("{events:?}");
    };
    let acme = entries
        .iter()
        .find(|e| e.source.ends_with("acme.toml"))
        .unwrap();
    let preset = acme.preset.as_ref().unwrap();
    assert_eq!(preset.label(), "Acme");
    let secret = preset.client_secret.as_deref().unwrap();
    assert!(
        ctx.preset_source(secret)
            .is_some_and(|source| source.ends_with("acme.toml")),
        "{secret}"
    );
    assert!(
        !secret.contains("GOCSPX"),
        "the TUI never sees the preset's secret"
    );
}

#[test]
fn a_preset_stand_in_names_the_preset_and_op_references_stay_references() {
    let ctx = test_ctx();
    let stand_in = ctx.preset_secret("/etc/rail/presets/acme.toml");
    assert_eq!(
        add_request(&ctx, &account("a@x.com", &stand_in)).client_secret,
        Some(ClientSecretInput::Preset(
            "/etc/rail/presets/acme.toml".into()
        ))
    );
    let reference = account("a@x.com", "op://Rail/google/client_secret");
    assert!(matches!(
        add_request(&ctx, &reference).client_secret,
        Some(ClientSecretInput::Reference(_))
    ));
}

#[test]
fn typed_text_that_looks_like_a_stand_in_is_sent_as_the_literal_secret() {
    let ctx = test_ctx();
    let stand_in = ctx.preset_secret("/etc/rail/presets/acme.toml");
    let prefix = stand_in.split_at(stand_in.len() - 4).0.to_string();
    for typed in [
        "rail-preset-secret:/etc/rail/presets/acme.toml".to_string(),
        "rail-preset-/etc/rail/presets/acme.toml".to_string(),
        prefix,
        format!("{stand_in} "),
    ] {
        assert_eq!(
            add_request(&ctx, &account("a@x.com", &typed)).client_secret,
            Some(ClientSecretInput::Value(rail_core::ops::Secret::new(
                typed.clone()
            ))),
            "{typed}"
        );
    }
    let other = test_ctx();
    assert!(
        matches!(
            add_request(&other, &account("a@x.com", &stand_in)).client_secret,
            Some(ClientSecretInput::Value(_))
        ),
        "a stand-in from another context is just text"
    );
}

#[tokio::test]
async fn read_preset_reports_a_parse_error_with_its_line() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let file = tmp.path().join("bad.toml");
    std::fs::write(&file, "kind = \"gmail\"\nnope = 1\n").unwrap();
    let (ctx, _) = daemon_ctx(&paths, None).await;
    let events = run(to_effect(Action::ReadPreset(file), &ctx)).await;
    let [Event::PresetFetched(Err(failure))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(
        failure.message.contains("bad.toml:2:"),
        "{}",
        failure.message
    );
}

#[tokio::test]
async fn read_preset_resolves_a_relative_path_against_the_working_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let file = tmp.path().join("acme.toml");
    std::fs::write(&file, ACME).unwrap();
    let typed = relative_to_cwd(&file);
    assert!(typed.is_relative(), "{}", typed.display());
    let (ctx, _) = daemon_ctx(&paths, None).await;
    let events = run(to_effect(Action::ReadPreset(typed), &ctx)).await;
    let [Event::PresetFetched(Ok(preset))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(preset.label(), "Acme");
    let stand_in = preset.client_secret.as_deref().unwrap();
    let source = ctx.preset_source(stand_in).unwrap();
    assert!(Path::new(&source).is_absolute(), "{source}");
    assert_eq!(std::fs::read_to_string(source).unwrap(), ACME);
}

#[tokio::test]
async fn fetch_effect_refuses_plain_http() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let (ctx, _) = daemon_ctx(&paths, None).await;
    let action = Action::FetchPreset("http://127.0.0.1:9/preset.toml".into());
    let events = run(to_effect(action, &ctx)).await;
    let [Event::PresetFetched(Err(failure))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(
        failure.message.contains("must be an https:// URL"),
        "{}",
        failure.message
    );
}

#[tokio::test]
async fn open_url_uses_the_portal_opener() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let opener: Opener = Arc::new(move |url: &Url| {
        let _ = tx.send(url.clone());
        Ok(())
    });
    let ctx = EffectContext::with(opener, test_ctx().daemon);
    let url = Url::parse("https://portal.azure.com/").unwrap();
    assert!(
        run(to_effect(Action::OpenUrl(url.clone()), &ctx))
            .await
            .is_empty()
    );
    assert_eq!(rx.try_recv().unwrap(), url);
}

#[tokio::test]
async fn run_login_refuses_a_configured_address_before_the_browser() {
    let tmp = tempfile::tempdir().unwrap();
    let config = format!(
        "{FILE_BACKEND}\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"x\"\n"
    );
    let paths = tmp_paths(&tmp, &config);
    let (ctx, mut urls) = daemon_ctx(&paths, None).await;
    let action = Action::RunLogin(account("ME@example.com", "sec"));
    let events = run(to_effect(action, &ctx)).await;
    let [Event::LoginFinished(Err(failure))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(
        failure.message.contains("already configured"),
        "{}",
        failure.message
    );
    assert_eq!(failure.class, ErrorClass::Permanent);
    assert!(urls.try_recv().is_err(), "no browser for a refused account");
}

#[tokio::test]
async fn a_login_through_the_daemon_saves_and_persist_reports_it() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let server = token_server("A1", "R1").await;
    let (ctx, urls) = daemon_ctx(&paths, Some(format!("{}/token", server.uri()))).await;
    let browser = redirect_when_asked(urls);
    let events = run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    ))
    .await;
    browser.await.unwrap();
    let [Event::LoginUrl(_), Event::LoginFinished(Ok(tokens))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(
        tokens.access_token.is_empty() && tokens.refresh_token.is_empty(),
        "the real tokens stay in the daemon"
    );
    let secrets = FileSecretStore::new(paths.secrets_file());
    assert_eq!(
        secrets
            .get_tokens("me@example.com")
            .unwrap()
            .unwrap()
            .access_token,
        "A1"
    );
    assert_eq!(
        Store::open(&paths.db_file())
            .unwrap()
            .list_accounts()
            .unwrap()
            .len(),
        1
    );
    let persist = Action::Persist {
        account: account("me@example.com", "sec"),
        tokens: tokens.clone(),
    };
    let events = run(to_effect(persist, &ctx)).await;
    let [Event::Persisted(Ok(report))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(report.config_path, paths.config_file());
    assert_eq!(report.token_backend, "file");
    assert_eq!(
        report.token_location,
        paths.secrets_file().display().to_string()
    );
    assert_eq!(pending_in_daemon(&ctx).await, 0);
}

#[tokio::test]
async fn persist_without_a_saved_login_is_a_failure() {
    let ctx = test_ctx();
    let persist = Action::Persist {
        account: account("me@example.com", "sec"),
        tokens: saved_by_the_daemon(),
    };
    let events = run(to_effect(persist, &ctx)).await;
    assert!(
        matches!(events.as_slice(), [Event::Persisted(Err(_))]),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_failed_save_in_the_daemon_is_a_login_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let server = token_server("A1", "R1").await;
    let (ctx, mut urls) = daemon_ctx(&paths, Some(format!("{}/token", server.uri()))).await;
    let job = tokio::spawn(run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    )));
    let url = tokio::time::timeout(Duration::from_secs(10), urls.recv())
        .await
        .unwrap()
        .unwrap();
    let taken = format!(
        "{FILE_BACKEND}\n[[accounts]]\nname = \"other\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"x\"\n"
    );
    std::fs::write(paths.config_file(), &taken).unwrap();
    redirect(&url).await;
    let events = job.await.unwrap();
    let Some(Event::LoginFinished(Err(failure))) = events.last() else {
        panic!("{events:?}");
    };
    assert!(
        failure.message.contains("already configured"),
        "{}",
        failure.message
    );
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), taken);
}

#[tokio::test]
async fn an_aborted_login_job_cancels_the_login_in_the_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let (ctx, mut urls) = daemon_ctx(&paths, None).await;
    let job = tokio::spawn(run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    )));
    let url = tokio::time::timeout(Duration::from_secs(10), urls.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(!query(&url, "state").is_empty());
    assert_eq!(pending_in_daemon(&ctx).await, 1);
    job.abort();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pending_in_daemon(&ctx).await != 0 {
        assert!(
            Instant::now() < deadline,
            "the cancel never reached the daemon"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(ctx.settle().await, Settled::Clean);
    assert!(!paths.secrets_file().exists(), "no tokens");
}

#[tokio::test]
async fn settle_cancels_a_login_still_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let (ctx, mut urls) = daemon_ctx(&paths, None).await;
    let job = tokio::spawn(run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    )));
    tokio::time::timeout(Duration::from_secs(10), urls.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ctx.settle().await, Settled::Clean);
    assert_eq!(pending_in_daemon(&ctx).await, 0);
    let events = job.await.unwrap();
    assert!(
        matches!(events.last(), Some(Event::LoginFinished(Err(_)))),
        "{events:?}"
    );
}

#[tokio::test]
async fn settle_gives_up_on_a_daemon_that_does_not_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    rail_core::paths::create_private_dir(&paths.runtime_dir).unwrap();
    let listener = tokio::net::UnixListener::bind(paths.socket_file()).unwrap();
    let silent = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let spawn: SpawnFn = Arc::new(|| Ok(()));
    let connector = Connector::new(paths, spawn).with_start_timeout(Duration::from_secs(30));
    let ctx = EffectContext::with(test_ctx().opener, connector)
        .with_cancel_timeout(Duration::from_millis(200));
    lock(&ctx.outstanding).ids.insert(PendingId("p1".into()));
    let started = Instant::now();
    assert_eq!(ctx.settle().await, Settled::Clean);
    assert!(started.elapsed() < Duration::from_secs(5), "settle hung");
    silent.abort();
}

#[tokio::test]
async fn a_quit_while_the_daemon_saves_reports_the_account_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let server = token_server("A1", "R1").await;
    let token_url = Some(format!("{}/token", server.uri()));
    let (ctx, urls, gates) = gated_daemon_ctx(&paths, token_url).await;
    gates.save.armed.store(true, Ordering::SeqCst);
    let job = tokio::spawn(run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    )));
    let browser = redirect_when_asked(urls);
    tokio::time::timeout(Duration::from_secs(10), gates.save.reached.notified())
        .await
        .expect("the daemon reached its save");
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert_eq!(ctx.settle().await, Settled::SaveKept);
    gates.save.release.notify_one();
    browser.await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while pending_in_daemon(&ctx).await != 0 {
        assert!(Instant::now() < deadline, "the save never finished");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let config = rail_core::config::load(&paths.config_file()).unwrap();
    assert_eq!(
        config.accounts[0].address, "me@example.com",
        "the save was kept"
    );
}

#[tokio::test]
async fn an_abort_while_the_begin_reply_is_in_flight_cancels_the_login() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = tmp_paths(&tmp, FILE_BACKEND);
    let server = token_server("A1", "R1").await;
    let token_url = Some(format!("{}/token", server.uri()));
    let (ctx, _urls, gates) = gated_daemon_ctx(&paths, token_url).await;
    gates.begin.armed.store(true, Ordering::SeqCst);
    let job = tokio::spawn(run(to_effect(
        Action::RunLogin(account("me@example.com", "sec")),
        &ctx,
    )));
    tokio::time::timeout(Duration::from_secs(10), gates.begin.reached.acquire())
        .await
        .expect("the begin reached its gate")
        .unwrap()
        .forget();
    let url = gates.begin.urls.lock().unwrap()[0].clone();
    job.abort();
    assert!(
        job.await.unwrap_err().is_cancelled(),
        "aborted before the reply"
    );
    gates.begin.open.send_replace(true);
    assert_eq!(ctx.settle().await, Settled::Clean);
    assert_eq!(
        pending_in_daemon(&ctx).await,
        0,
        "the begun login was cancelled"
    );
    // A browser coming back now finds nothing to complete.
    let late = format!(
        "{}/?code=c0de&state={}",
        query(&url, "redirect_uri"),
        query(&url, "state")
    );
    let _ = tokio::time::timeout(Duration::from_secs(5), reqwest::get(late)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!paths.secrets_file().exists(), "no tokens");
    assert_eq!(
        std::fs::read_to_string(paths.config_file()).unwrap(),
        FILE_BACKEND
    );
}

#[test]
fn operation_errors_become_failures_with_their_class_and_hint() {
    let transient =
        OpError::new(ErrorCode::Secrets, ErrorClass::Transient, "disk busy").with_hint("try again");
    let shown = failure(&transient);
    assert_eq!(shown.class, ErrorClass::Transient);
    assert_eq!(shown.message, "disk busy\nhint: try again");
    assert!(!shown.consent_required);
    let consent = OpError::new(
        ErrorCode::ConsentRequired,
        ErrorClass::Permanent,
        "AADSTS65001",
    );
    assert!(failure(&consent).consent_required);
}

#[test]
fn outside_text_in_a_failure_is_escaped_and_labelled_on_its_own_line() {
    let error = OpError::new(
        ErrorCode::Auth,
        ErrorClass::Permanent,
        "the provider refused the login",
    )
    .with_detail("denied\n\u{1b}[2J")
    .with_hint("try again");
    let shown = failure(&error);
    let lines: Vec<&str> = shown.message.lines().collect();
    assert_eq!(lines[0], "the provider refused the login");
    assert!(lines[1].starts_with("from outside Rail"), "{}", lines[1]);
    assert!(lines[1].contains(r#""denied\n\u{1b}[2J""#), "{}", lines[1]);
    assert_eq!(lines[2], "hint: try again");
    assert!(!shown.message.contains('\u{1b}'));
}

/// The setup wizard's Keys step writes `keymap` and keeps the rest of the config.
#[test]
fn save_keymap_writes_the_preset_and_keeps_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# mine\n[general]\nhints = true\n").unwrap();
    super::save_keymap(&path, rail_core::config::KeymapPreset::Vim).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("keymap = \"vim\""), "{text}");
    assert!(
        text.contains("# mine") && text.contains("hints = true"),
        "{text}"
    );
}

/// A preset the config's own `[keys]` clash with is refused, naming the entry, and the file
/// is left alone: written, it would stop Rail from starting.
#[test]
fn save_keymap_refuses_a_preset_keys_clash_with() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let text = "[keys]\n\"list.star\" = \"U\"\n";
    std::fs::write(&path, text).unwrap();
    let error = super::save_keymap(&path, rail_core::config::KeymapPreset::Vim).unwrap_err();
    assert!(error.contains("list.star"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}
