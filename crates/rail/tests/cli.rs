use std::process::Command;

/// Point a command's five XDG-style directories under `dir`, so no test ever reads or
/// writes the real config, store, secrets or daemon socket, and give it the stand-in browser
/// (recording into `opened_url_file`), so no test, and no daemon a test starts, can ever open
/// the real one. A test may set its own `RAIL_TEST_URL_FILE`. The daemon a command starts
/// inherits this environment and exits two seconds after its last client leaves.
fn rail_env<'a>(cmd: &'a mut Command, dir: &tempfile::TempDir) -> &'a mut Command {
    cmd.env("RAIL_TEST_OPENER", write_opener(dir))
        .env("RAIL_TEST_URL_FILE", opened_url_file(dir))
        .env("RAIL_CONFIG_DIR", dir.path().join("config"))
        .env("RAIL_DATA_DIR", dir.path().join("data"))
        .env("RAIL_CACHE_DIR", dir.path().join("cache"))
        .env("RAIL_STATE_DIR", dir.path().join("state"))
        .env("RAIL_RUNTIME_DIR", dir.path().join("run"))
        // `rail doctor` looks for the login unit here, never in the real one.
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        // On macOS the launchd agent lives under HOME, so there HOME is the test's too.
        .envs(cfg!(target_os = "macos").then(|| ("HOME", dir.path().join("home"))))
        .env("RAIL_TEST_IDLE_SECS", "2")
        // A configured account in a test must never reach Google or Microsoft, and its daemon must still
        // idle out. Tests that serve a token endpoint set RAIL_TEST_TOKEN_URL again later.
        .env(
            "RAIL_TEST_GMAIL_URL",
            "http://127.0.0.1:9/gmail/v1/users/me/",
        )
        .env("RAIL_TEST_GRAPH_URL", "http://127.0.0.1:9/v1.0/")
        .env("RAIL_TEST_TOKEN_URL", "http://127.0.0.1:9/token")
        .env("RAIL_TEST_IDLE_WITH_ACCOUNTS", "1")
        // A pseudo-terminal answers no picture query, which would wait out its timeout.
        .env("RAIL_TEST_NO_IMAGE_QUERY", "1")
}

/// Runs `rail daemon stop` for `dir` when dropped, so a test that starts a daemon ends it,
/// even when the test fails half way.
struct StopsDaemon<'a>(&'a tempfile::TempDir);

impl Drop for StopsDaemon<'_> {
    fn drop(&mut self) {
        let _ = rail_cmd(self.0).args(["daemon", "stop"]).output();
    }
}

/// Wait for `child` at most `limit`, failing (and killing it) if it is still running then.
fn finish_within(child: std::process::Child, limit: std::time::Duration) -> std::process::Output {
    let mut running = KillsChild(Some(child));
    let deadline = std::time::Instant::now() + limit;
    while running.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "still running after {limit:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    running.0.take().unwrap().wait_with_output().unwrap()
}

/// Kills a spawned `rail` if the test fails before it is waited on.
struct KillsChild(Option<std::process::Child>);

impl Drop for KillsChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A `rail` command confined to `dir` by `rail_env`.
fn rail_cmd(dir: &tempfile::TempDir) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rail"));
    rail_env(&mut cmd, dir);
    cmd
}

#[test]
fn version_flag_prints_name_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let out = rail_cmd(&dir).arg("--version").output().expect("run rail");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "rail 0.8.0\n");
}

#[test]
fn rail_with_a_config_but_no_account_opens_the_wizard_which_needs_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, "[general]\nsecrets = \"file\"\n");
    let out = rail_cmd(&dir).output().expect("run rail");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("setup wizard needs an interactive terminal"),
        "got {stderr:?}"
    );
    assert!(!stderr.contains("not built yet"), "got {stderr:?}");
}

#[test]
fn rail_with_an_account_in_a_pipe_says_the_mail_screen_needs_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    let out = bounded(&mut rail_cmd(&dir));
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mail screen needs an interactive terminal"),
        "got {stderr:?}"
    );
}

#[test]
fn a_bad_keys_entry_stops_rail_with_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(
        &dir,
        &format!("{WORK_FILE_BACKEND}\n[keys]\n\"list.write\" = \"e\"\n"),
    );
    let out = bounded(rail_cmd(&dir).args(["setup"]));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("config.toml:12:"), "got {stderr:?}");
    assert!(
        stderr.contains("\"list.write\" is not an action Rail knows"),
        "got {stderr:?}"
    );
}

#[test]
fn a_bad_keys_entry_stops_plain_rail_with_its_line_too() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(
        &dir,
        &format!("{WORK_FILE_BACKEND}\n[keys]\n\"list.write\" = \"e\"\n"),
    );
    let out = bounded(&mut rail_cmd(&dir));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("config.toml:12:"), "got {stderr:?}");
}

#[test]
fn account_list_without_config_explains_how_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let out = rail_cmd(&dir).args(["account", "list"]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("rail account add"), "got {stderr:?}");
}

#[test]
fn account_add_refuses_duplicate_address_before_opening_a_browser() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_file = config_dir.join("config.toml");
    // The file backend: the daemon syncs this account, and must never read the real keyring.
    let original = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"x\"\n";
    std::fs::write(&config_file, original).unwrap();

    let out = bounded(rail_cmd(&dir).args([
        "account",
        "add",
        "gmail",
        "--address",
        "ME@example.com",
        "--client-id",
        "y",
    ]));

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already configured"), "got {stderr:?}");
    assert!(stderr.contains("config.toml"), "got {stderr:?}");
    // Refused before any login: no URL printed, config untouched, no store created.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("http"), "got {stdout:?}");
    assert_eq!(std::fs::read_to_string(&config_file).unwrap(), original);
    // The daemon opens the store for the configured account; the refusal wrote no tokens.
    assert!(!dir.path().join("data").join("secrets.json").exists());
    assert!(!opened_url_file(&dir).exists(), "the browser was asked");
}

#[test]
fn account_add_refuses_duplicate_name_before_opening_a_browser() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_file = config_dir.join("config.toml");
    // The file backend: the daemon syncs this account, and must never read the real keyring.
    let original = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"x\"\n";
    std::fs::write(&config_file, original).unwrap();

    let out = bounded(rail_cmd(&dir).args([
        "account",
        "add",
        "gmail",
        "--address",
        "other@example.com",
        "--client-id",
        "x",
        "--name",
        "work",
    ]));

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("work"), "got {stderr:?}");
    assert!(
        stderr.to_lowercase().contains("duplicate"),
        "got {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("http"), "got {stdout:?}");
    assert_eq!(std::fs::read_to_string(&config_file).unwrap(), original);
    // The daemon opens the store for the configured account; the refusal wrote no tokens.
    assert!(!dir.path().join("data").join("secrets.json").exists());
    assert!(!opened_url_file(&dir).exists(), "the browser was asked");
}

#[test]
fn doctor_exits_nonzero_without_config() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let out = rail_cmd(&dir).arg("doctor").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FAIL config"), "got {stdout:?}");
    assert!(stdout.contains("rail account add"), "got {stdout:?}");
    assert!(stdout.contains("no config file at"), "got {stdout:?}");
    assert!(!stdout.contains("ok   secrets"), "got {stdout:?}");
    assert!(stdout.contains("ok   store"), "got {stdout:?}");
    assert!(
        dir.path().join("state").join("rail.log").is_file(),
        "log file created"
    );
}

#[test]
fn account_add_fails_before_the_browser_when_secrets_file_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config_dir = dir.path().join("config");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = "[general]\nsecrets = \"file\"\n";
    std::fs::write(config_dir.join("config.toml"), config).unwrap();
    std::fs::write(data_dir.join("secrets.json"), "{not json").unwrap();
    let out = bounded(rail_cmd(&dir).args([
        "account",
        "add",
        "gmail",
        "--address",
        "new@example.com",
        "--client-id",
        "x",
    ]));
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("corrupt"), "got {stderr:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("http"), "got {stdout:?}");
    assert_eq!(
        std::fs::read_to_string(config_dir.join("config.toml")).unwrap(),
        config
    );
    assert!(!opened_url_file(&dir).exists(), "the browser was asked");
}

#[test]
fn panic_inside_the_alternate_screen_restores_the_terminal_before_the_message() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("terminal.txt");
    let file = std::fs::File::create(&log).unwrap();
    // stdout and stderr share one file description, so the file keeps their real order.
    let status = rail_cmd(&dir)
        .arg("--panic-test")
        .env("RUST_BACKTRACE", "0")
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .status()
        .unwrap();
    assert!(!status.success());
    let out = String::from_utf8_lossy(&std::fs::read(&log).unwrap()).into_owned();
    let entered = out
        .find("\x1b[?1049h")
        .expect("entered the alternate screen");
    let left = out.rfind("\x1b[?1049l").expect("left the alternate screen");
    let message = out.find("rail panic test").expect("panic message printed");
    assert!(entered < left && left < message, "{out:?}");
}

const WORK_FILE_BACKEND: &str = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"cid\"\nclient_secret = \"sec\"\n";

fn write_config(dir: &tempfile::TempDir, text: &str) -> std::path::PathBuf {
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let file = config_dir.join("config.toml");
    std::fs::write(&file, text).unwrap();
    file
}

/// A browser stand-in: writes the URL it was given to `$RAIL_TEST_URL_FILE` (atomically,
/// so a reader never sees half of it) and exits. Created once per directory, by rename, so a
/// daemon running it never meets a half-written script.
fn write_opener(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join("opener.sh");
    if path.exists() {
        return path;
    }
    let script = "#!/bin/sh\nprintf '%s' \"$1\" > \"$RAIL_TEST_URL_FILE.tmp\"\nmv \"$RAIL_TEST_URL_FILE.tmp\" \"$RAIL_TEST_URL_FILE\"\n";
    let temp = dir
        .path()
        .join(format!("opener.{:?}.tmp", std::thread::current().id()));
    std::fs::write(&temp, script).unwrap();
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&temp, &path).unwrap();
    path
}

/// Where the default stand-in browser records a URL; it exists only if a browser was asked.
fn opened_url_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("opened-url.txt")
}

/// Run `cmd` with its output captured, failing if it takes longer than half a minute.
fn bounded(cmd: &mut Command) -> std::process::Output {
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    finish_within(child, std::time::Duration::from_secs(30))
}

#[cfg(debug_assertions)]
fn query(url: &url::Url, key: &str) -> String {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{key} missing from {url}"))
}

#[cfg(all(unix, debug_assertions))]
async fn wait_for_url(file: &std::path::Path) -> url::Url {
    for _ in 0..500 {
        if let Ok(text) = std::fs::read_to_string(file) {
            return url::Url::parse(&text).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the opener was never called");
}

#[test]
fn account_login_unknown_account_fails_before_the_opener() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let url_file = dir.path().join("url.txt");
    let out = bounded(
        rail_cmd(&dir)
            .args(["account", "login", "nobody"])
            .env("RAIL_TEST_URL_FILE", &url_file),
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no account named or addressed"),
        "got {stderr:?}"
    );
    assert!(!url_file.exists(), "the opener must not run");
    assert_eq!(std::fs::read_to_string(config).unwrap(), WORK_FILE_BACKEND);
    assert!(!dir.path().join("data").join("secrets.json").exists());
}

#[cfg(unix)]
// Release builds skip the real-browser flows: they must never launch a browser.
#[cfg(debug_assertions)]
#[tokio::test]
async fn account_login_replaces_tokens_in_the_file_backend() {
    use rail_core::auth::{FileSecretStore, SecretStore, TokenSet};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let secrets = FileSecretStore::new(dir.path().join("data").join("secrets.json"));
    let old = TokenSet {
        access_token: "old-access".into(),
        refresh_token: "old-refresh".into(),
        expires_at: 1,
    };
    secrets.set_tokens("me@example.com", &old).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("code=the-code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new-access", "refresh_token": "new-refresh",
            "expires_in": 3600, "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@example.com"
        })))
        .mount(&server)
        .await;

    let url_file = dir.path().join("url.txt");
    let child = rail_cmd(&dir)
        .args(["account", "login", "WORK"])
        .env("RAIL_TEST_OPENER", write_opener(&dir))
        .env("RAIL_TEST_URL_FILE", &url_file)
        .env("RAIL_TEST_TOKEN_URL", format!("{}/token", server.uri()))
        .env(
            "RAIL_TEST_GMAIL_URL",
            format!("{}/gmail/v1/users/me/", server.uri()),
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let consent = wait_for_url(&url_file).await;
    let state = query(&consent, "state");
    let redirect = query(&consent, "redirect_uri");
    reqwest::get(format!("{redirect}/?code=the-code&state={state}"))
        .await
        .unwrap();
    let out = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .unwrap()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Logged in to me@example.com"),
        "got {stdout:?}"
    );
    let tokens = secrets.get_tokens("me@example.com").unwrap().unwrap();
    assert_eq!(tokens.access_token, "new-access");
    assert_eq!(tokens.refresh_token, "new-refresh");
    assert_eq!(std::fs::read_to_string(config).unwrap(), WORK_FILE_BACKEND);
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn account_list_keeps_columns_aligned_for_long_names() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"a-very-long-account-name\"\nkind = \"gmail\"\naddress = \"someone.with.a.very.long.address@example.com\"\nclient_id = \"c\"\n\n[[accounts]]\nname = \"w\"\nkind = \"m365\"\naddress = \"w@x.io\"\nclient_id = \"c\"\n";
    write_config(&dir, config);
    let out = rail_cmd(&dir).args(["account", "list"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    let tokens = lines[0].find("TOKENS").unwrap();
    assert_eq!(lines[1].find("missing"), Some(tokens), "{text}");
    assert_eq!(lines[2].find("missing"), Some(tokens), "{text}");
}

#[test]
fn doctor_says_1_account() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    let out = rail_cmd(&dir).arg("doctor").output().unwrap();
    let text = stdout_of(&out);
    assert!(text.contains("(1 account)"), "{text}");
    assert!(!text.contains("1 accounts"), "{text}");
}

/// Tokens for `me@example.com` in the file backend, so a removal test can see they stay.
fn store_work_tokens(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use rail_core::auth::{FileSecretStore, SecretStore, TokenSet};
    let file = dir.path().join("data").join("secrets.json");
    let tokens = TokenSet {
        access_token: "a".into(),
        refresh_token: "r".into(),
        expires_at: 1,
    };
    FileSecretStore::new(file.clone())
        .set_tokens("me@example.com", &tokens)
        .unwrap();
    file
}

/// Without a terminal nothing is removed, whatever the account. `output()` pipes
/// stdout and gives no terminal on stdin.
#[test]
fn account_remove_without_a_terminal_refuses_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let secrets = store_work_tokens(&dir);
    let before = std::fs::read_to_string(&secrets).unwrap();
    for who in ["work", "ME@example.com", "nobody"] {
        let out = rail_cmd(&dir)
            .args(["account", "remove", who])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{who}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("run `rail account remove` yourself in a terminal"),
            "{who}: {stderr}"
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), WORK_FILE_BACKEND);
        assert_eq!(std::fs::read_to_string(&secrets).unwrap(), before);
    }
}

/// Final review F4: `rail secrets set` without a terminal changes nothing, and says to run
/// it in one. No daemon is started: the check comes first.
#[test]
fn secrets_set_without_a_terminal_refuses_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&dir, WORK_FILE_BACKEND);
    for backend in ["keyring", "file", "1password"] {
        let out = rail_cmd(&dir)
            .args(["secrets", "set", backend, "--vault", "Rail"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{backend}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("run `rail secrets set` yourself in a terminal"),
            "{backend}: {stderr}"
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), WORK_FILE_BACKEND);
    }
    assert!(!dir.path().join("run").join("rail.sock").exists());
}

#[test]
fn account_remove_has_no_yes_flag() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let out = rail_cmd(&dir)
        .args(["account", "remove", "work", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), WORK_FILE_BACKEND);
}

/// Ctrl-C (SIGINT) while a flag login waits on the browser sends `pending.cancel`: the command
/// says so, nothing is saved and the daemon holds nothing pending afterwards.
#[cfg(unix)]
#[cfg(debug_assertions)]
#[tokio::test]
async fn ctrl_c_during_a_flag_login_cancels_it_and_saves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let url_file = dir.path().join("url.txt");
    let child = rail_cmd(&dir)
        .args(["account", "login", "work"])
        .env("RAIL_TEST_OPENER", write_opener(&dir))
        .env("RAIL_TEST_URL_FILE", &url_file)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut running = KillsChild(Some(child));
    wait_for_url(&url_file).await;
    let pid = running.0.as_ref().unwrap().id();
    let interrupted = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(interrupted.success());
    let child = running.0.take().unwrap();
    let out = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("login cancelled; nothing was saved"),
        "{stderr}"
    );
    assert_eq!(std::fs::read_to_string(config).unwrap(), WORK_FILE_BACKEND);
    assert!(!dir.path().join("data").join("secrets.json").exists());
    let status = rail_cmd(&dir).args(["daemon", "status"]).output().unwrap();
    assert!(stdout_of(&status).contains(" 0 pending,"), "{status:?}");
    rail_cmd(&dir).args(["daemon", "stop"]).output().unwrap();
}

/// Review focus 1: the wizard never starts in a pipe. `output()` pipes stdout.
fn assert_pipe_hint(dir: &tempfile::TempDir, args: &[&str]) {
    let out = rail_cmd(dir).args(args).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("interactive terminal"),
        "{args:?}: {stderr:?}"
    );
    assert!(
        stderr.contains("rail account add <gmail|m365> --address"),
        "{args:?}: {stderr:?}"
    );
    assert!(
        out.stdout.is_empty(),
        "{args:?}: nothing on stdout, not even escape codes"
    );
    assert!(
        !dir.path().join("config").join("config.toml").exists(),
        "{args:?}"
    );
    assert!(!dir.path().join("data").exists(), "{args:?}");
}

#[test]
fn setup_refuses_to_start_when_stdout_is_piped() {
    let dir = tempfile::tempdir().unwrap();
    assert_pipe_hint(&dir, &["setup"]);
}

#[test]
fn bare_account_add_with_piped_stdout_prints_the_hint() {
    let dir = tempfile::tempdir().unwrap();
    assert_pipe_hint(&dir, &["account", "add"]);
}

#[test]
fn account_add_with_only_a_provider_prints_the_hint() {
    let dir = tempfile::tempdir().unwrap();
    assert_pipe_hint(&dir, &["account", "add", "m365"]);
}

#[test]
fn rail_without_config_and_piped_stdout_prints_the_hint() {
    let dir = tempfile::tempdir().unwrap();
    assert_pipe_hint(&dir, &[]);
}

/// Run `rail account add <args>` and return (exit code, stderr).
fn add_failure(dir: &tempfile::TempDir, args: &[&str]) -> (Option<i32>, String) {
    let out = bounded(rail_cmd(dir).args(["account", "add"]).args(args));
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn account_add_flags_still_need_a_provider_and_a_client_id() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stderr) = add_failure(&dir, &["--address", "a@b.c", "--client-id", "x"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("provider"), "{stderr:?}");
    let (code, stderr) = add_failure(&dir, &["gmail", "--address", "a@b.c"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("--client-id"), "{stderr:?}");
    let (code, stderr) = add_failure(&dir, &["gmail", "--tenant", "t"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("--address"), "{stderr:?}");
    assert!(!dir.path().join("data").exists());
}

#[test]
fn doctor_without_config_mentions_rail_setup() {
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let out = rail_cmd(&dir).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("run `rail setup`"), "got {stdout:?}");
    assert!(
        stdout.contains("\n              hint: run `rail setup`, or `rail account add`\n"),
        "got {stdout:?}"
    );
    assert!(!stdout.contains("rail_account_add"), "got {stdout:?}");
}

/// Saving an attachment from the reader, in a pseudo-terminal: the same two gates `mod pty`
/// carries, whose helpers it reuses.
#[cfg(target_os = "linux")]
#[cfg(debug_assertions)]
#[path = "cli/attachments.rs"]
mod attachments;

/// Bulk selection end to end, in a pseudo-terminal like `pty`: `* a` then `e`, a Shift range
/// then `e`, and `z` within the grace, against a mocked Gmail. Gated like `pty`, whose helpers
/// it reuses.
#[cfg(target_os = "linux")]
#[cfg(debug_assertions)]
#[path = "cli/bulk.rs"]
mod bulk;

#[path = "cli/daemon.rs"]
mod daemon;

/// The flows that need a terminal, run in a pseudo-terminal: the wizard's Ctrl-C during the
/// login wait, and the question `rail account remove` asks.
#[cfg(target_os = "linux")]
// Release builds skip the pty flows: the test opener is debug-only, so they would launch a real browser.
#[cfg(debug_assertions)]
#[path = "cli/pty.rs"]
mod pty;

/// Saved searches from outside Rail: `rail doctor` on a hand-written table, and the
/// whole save story in a pseudo-terminal (that test is Linux-only, like `pty`).
#[path = "cli/saved.rs"]
mod saved;

#[path = "cli/service.rs"]
mod service;

/// `h` snoozing a thread end to end, in a pseudo-terminal like the flows above (Linux, debug).
#[cfg(target_os = "linux")]
#[cfg(debug_assertions)]
#[path = "cli/snooze.rs"]
mod snooze;

#[path = "cli/sync.rs"]
mod sync;
