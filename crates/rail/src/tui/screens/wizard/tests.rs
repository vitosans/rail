use super::*;
use std::io;
use std::time::Duration;

use crossterm::event::Event as TermEvent;
use rail_core::auth::{FileSecretStore, TokenSet};
use rail_core::error::ErrorClass;
use rail_core::onboarding::{Busy, DoneReport, Failure};
use rail_core::paths::Paths;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tokio::sync::mpsc;
use url::Url;

use crate::tui::app::TaskSink;
use crate::tui::app::{App, update};
use crate::tui::run::run_loop;
use crate::tui::test_util::{
    RecordingClipboard, ctrl, daemon_ctx, feed, gmail_own_app_keys, key, logged, press,
    redirect_when_asked, token_server, type_text, wizard_screen,
};

fn drive(screen: &mut WizardScreen, keys: Vec<KeyEvent>) -> Vec<Effect> {
    keys.into_iter()
        .flat_map(|k| screen.update(Msg::Key(k)))
        .collect()
}

fn tokens() -> TokenSet {
    TokenSet {
        access_token: "a".into(),
        refresh_token: "r".into(),
        expires_at: 1,
    }
}

fn report() -> DoneReport {
    DoneReport {
        config_path: "/c/config.toml".into(),
        token_backend: "file".into(),
        token_location: "/d/secrets.json".into(),
        store_path: "/d/rail.db".into(),
    }
}

#[test]
fn enter_on_a_choice_moves_through_the_steps() {
    let mut screen = wizard_screen();
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Provider);
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Instructions);
    assert_eq!(
        screen.wizard.draft().kind,
        Some(rail_core::config::AccountKind::M365)
    );
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.wizard.step(), Step::Provider);
}

#[test]
fn typing_edits_the_draft_and_q_is_text_in_a_field() {
    let mut screen = wizard_screen();
    drive(
        &mut screen,
        vec![
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
        ],
    );
    assert_eq!(screen.wizard.step(), Step::Details);
    assert!(screen.captures_text());
    type_text(&mut screen, "q@x.io");
    assert_eq!(screen.wizard.draft().address, "q@x.io");
    assert_eq!(
        screen.wizard.draft().name,
        "q",
        "the name follows the address"
    );
    press(&mut screen, KeyCode::Tab);
    assert_eq!(
        screen.inputs[&Field::Name].value(),
        "q",
        "programmatic changes reach the input"
    );
}

/// `general.field_keys` reaches the wizard's fields: `Ctrl-w` removes a word with the
/// readline keys and nothing with the basic ones.
#[test]
fn the_fields_edit_with_the_configs_field_keys() {
    for (keys, left) in [
        (rail_core::config::FieldKeys::Readline, "q@x.io "),
        (rail_core::config::FieldKeys::Basic, "q@x.io no"),
    ] {
        let mut screen = wizard_screen().with_field_keys(keys);
        drive(
            &mut screen,
            vec![
                key(KeyCode::Down),
                key(KeyCode::Enter),
                key(KeyCode::Enter),
                key(KeyCode::Enter),
            ],
        );
        type_text(&mut screen, "q@x.io no");
        screen.update(Msg::Key(ctrl('w')));
        assert_eq!(screen.wizard.draft().address, left, "{keys:?}");
    }
}

/// In a vi field `Esc` goes to normal mode first; only then does it go back a step.
#[test]
fn esc_in_a_vi_field_goes_to_normal_mode_before_going_back() {
    let mut screen = wizard_screen().with_field_keys(rail_core::config::FieldKeys::Vi);
    drive(
        &mut screen,
        vec![
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
        ],
    );
    type_text(&mut screen, "q@x.io no");
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.wizard.step(), Step::Details, "normal mode, not back");
    type_text(&mut screen, "bD");
    assert_eq!(screen.wizard.draft().address, "q@x.io ");
    press(&mut screen, KeyCode::Esc);
    assert_ne!(
        screen.wizard.step(),
        Step::Details,
        "the second Esc goes back"
    );
}

#[test]
fn space_toggles_the_administrator_box() {
    let mut screen = wizard_screen();
    drive(
        &mut screen,
        vec![
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
        ],
    );
    assert_eq!(screen.wizard.step(), Step::Details);
    for _ in 0..6 {
        press(&mut screen, KeyCode::Tab);
    }
    assert!(!screen.captures_text(), "the checkbox takes no text");
    press(&mut screen, KeyCode::Char(' '));
    assert!(screen.wizard.draft().admin);
}

#[test]
fn the_login_starts_a_job_and_esc_cancels_it() {
    let mut screen = wizard_screen();
    let effects = drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    assert_eq!(screen.wizard.step(), Step::Login);
    assert!(
        matches!(effects.last(), Some(Effect::Spawn(_))),
        "{effects:?}"
    );
    let effects = press(&mut screen, KeyCode::Esc);
    assert!(
        matches!(effects.as_slice(), [Effect::CancelAll]),
        "{effects:?}"
    );
    assert_eq!(screen.wizard.step(), Step::Details);
}

#[test]
fn r_retries_and_c_copies_only_when_an_error_shows() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    assert!(
        press(&mut screen, KeyCode::Char('r')).is_empty(),
        "nothing to retry yet"
    );
    let failure = Failure::new("boom", ErrorClass::Transient);
    feed(&mut screen, Event::LoginFinished(Err(failure)));
    let effects = press(&mut screen, KeyCode::Char('r'));
    assert!(
        matches!(effects.as_slice(), [Effect::Spawn(_)]),
        "{effects:?}"
    );
}

/// Once saving, Esc and q do nothing, and Ctrl-C waits for the result.
#[test]
fn persist_cannot_be_cancelled() {
    let mut app = App::new(wizard_screen(), Box::new(RecordingClipboard::default()));
    for k in gmail_own_app_keys("me@gmail.com") {
        update(&mut app, Msg::Key(k));
    }
    let login = Msg::Task(TaskResult::Wizard(Event::LoginFinished(Ok(tokens()))));
    let effects = update(&mut app, login);
    assert!(
        matches!(effects.as_slice(), [Effect::Spawn(_)]),
        "{effects:?}"
    );
    assert_eq!(app.screen.wizard.busy(), Some(Busy::Persisting));
    for k in [key(KeyCode::Esc), key(KeyCode::Char('q')), ctrl('c')] {
        let effects = update(&mut app, Msg::Key(k));
        assert!(effects.is_empty(), "{k:?} gave {effects:?}");
        assert!(!app.quit, "{k:?} quit while saving");
    }
    assert_eq!(app.screen.wizard.step(), Step::Login);
    assert_eq!(app.screen.wizard.busy(), Some(Busy::Persisting));
    let saved = Msg::Task(TaskResult::Wizard(Event::Persisted(Ok(report()))));
    let effects = update(&mut app, saved);
    assert!(matches!(effects.as_slice(), [Effect::Quit]), "{effects:?}");
    assert_eq!(app.screen.added(), ["me".to_string()]);
}

/// A panicked job fails whatever action was in flight.
#[test]
fn a_panicked_job_fails_the_action_in_flight() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    assert_eq!(screen.wizard.busy(), Some(Busy::Login));
    screen.update(Msg::Task(TaskResult::JobPanicked("boom".into())));
    assert_eq!(screen.wizard.busy(), None);
    let failure = screen.wizard.failure().unwrap();
    assert_eq!(failure.message, "internal error: boom; see rail.log");
    assert_eq!(failure.class, ErrorClass::Permanent);
    let effects = press(&mut screen, KeyCode::Char('r'));
    assert!(
        matches!(effects.as_slice(), [Effect::Spawn(_)]),
        "{effects:?}"
    );
}

/// A panic message can quote a token or mail: the log gets its length, never its text.
#[test]
fn a_panicked_job_logs_its_length_not_its_text() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    let message = "secret-panic-text";
    let (_, log) = logged(|| screen.update(Msg::Task(TaskResult::JobPanicked(message.into()))));
    assert!(log.contains("a wizard job panicked"), "{log:?}");
    assert!(
        log.contains(&format!("message_bytes={}", message.len())),
        "{log:?}"
    );
    assert!(!log.contains(message), "{log:?}");
}

/// A panic while saving may come from a sibling job: the save keeps running, the screen
/// keeps holding, and the deferred quit still waits for the save's own result.
#[test]
fn a_panic_while_saving_is_ignored_and_the_save_still_finishes() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    feed(&mut screen, Event::LoginFinished(Ok(tokens())));
    assert!(screen.update(Msg::Key(ctrl('c'))).is_empty());
    let effects = screen.update(Msg::Task(TaskResult::JobPanicked("disk".into())));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(screen.wizard.busy(), Some(Busy::Persisting));
    assert!(screen.defers_quit(), "still saving");
    assert!(screen.wizard.failure().is_none());
    let effects = screen.update(Msg::Task(TaskResult::Wizard(Event::Persisted(
        Ok(report()),
    ))));
    assert!(matches!(effects.as_slice(), [Effect::Quit]), "{effects:?}");
    assert_eq!(screen.added(), ["me".to_string()]);
}

#[test]
fn a_panic_during_login_cancels_the_job_really_in_flight() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    assert_eq!(screen.wizard.busy(), Some(Busy::Login));
    let effects = screen.update(Msg::Task(TaskResult::JobPanicked("boom".into())));
    assert!(
        effects.iter().any(|e| matches!(e, Effect::CancelAll)),
        "{effects:?}"
    );
    assert!(screen.wizard.failure().is_some());
}

#[test]
fn c_on_the_login_step_copies_the_login_url() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    assert!(
        press(&mut screen, KeyCode::Char('c')).is_empty(),
        "no URL yet"
    );
    let login = Url::parse("https://accounts.example/auth?state=s").unwrap();
    feed(&mut screen, Event::LoginUrl(login.clone()));
    let effects = press(&mut screen, KeyCode::Char('c'));
    assert!(
        matches!(effects.as_slice(), [Effect::Copy(u)] if *u == login.to_string()),
        "{effects:?}"
    );
}

#[test]
fn the_copy_notice_for_the_login_url_does_not_promise_a_printout() {
    let mut screen = wizard_screen();
    drive(&mut screen, gmail_own_app_keys("me@gmail.com"));
    let login = Url::parse("https://accounts.example/auth?state=s").unwrap();
    feed(&mut screen, Event::LoginUrl(login));
    press(&mut screen, KeyCode::Char('c'));
    screen.update(Msg::Task(TaskResult::Copied(Ok(()))));
    let notice = screen.notice.as_deref().unwrap();
    assert!(!notice.contains("printed"), "{notice}");
}

#[test]
fn the_copy_notice_does_not_claim_success_it_cannot_verify() {
    let mut screen = wizard_screen();
    screen.update(Msg::Task(TaskResult::Copied(Ok(()))));
    assert_eq!(
        screen.notice.as_deref(),
        Some("Sent via OSC 52. If not copied, select the URL on screen")
    );
}

/// Review focus 2 and 5, in process: the real screen, loop and effects, cancelled while
/// the login waits for the browser, leave no config change, no tokens and no store row.
#[tokio::test]
async fn cancel_during_login_leaves_no_config_tokens_or_store_row() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    let config_text = "[general]\nsecrets = \"file\"\n";
    std::fs::write(paths.config_file(), config_text).unwrap();
    let (ctx, mut url_rx) = daemon_ctx(&paths, None).await;
    let config = rail_core::config::load(&paths.config_file()).unwrap();
    let screen = WizardScreen::new(
        Wizard::new(Some(config), None),
        ctx.clone(),
        crate::tui::test_util::keymap(),
    );
    let mut app = App::new(screen, Box::new(RecordingClipboard::default()));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (key_tx, mut key_rx) = mpsc::unbounded_channel::<io::Result<TermEvent>>();
    let events = futures_util::stream::poll_fn(move |cx| key_rx.poll_recv(cx));
    for k in gmail_own_app_keys("me@example.com") {
        key_tx.send(Ok(TermEvent::Key(k))).unwrap();
    }
    let driver = async move {
        let url = tokio::time::timeout(Duration::from_secs(10), url_rx.recv())
            .await
            .expect("the login reached the browser")
            .expect("the opener was called");
        assert!(url.query_pairs().any(|(k, _)| k == "state"), "{url}");
        key_tx.send(Ok(TermEvent::Key(ctrl('c')))).unwrap();
        key_tx
    };
    let mut handover = ();
    let (result, _keys) = tokio::join!(
        run_loop(
            &mut terminal,
            &mut app,
            crate::tui::test_util::once(events),
            &mut handover,
        ),
        driver
    );
    result.unwrap();
    assert_eq!(ctx.settle().await, crate::tui::effects::Settled::Clean);
    assert!(app.quit);
    assert!(app.screen.added().is_empty());
    assert_eq!(
        std::fs::read_to_string(paths.config_file()).unwrap(),
        config_text
    );
    assert!(!paths.secrets_file().exists(), "no tokens");
    assert!(!paths.db_file().exists(), "no store row");
    let secrets = FileSecretStore::new(paths.secrets_file());
    assert!(
        rail_core::auth::SecretStore::get_tokens(&secrets, "me@example.com")
            .unwrap()
            .is_none()
    );
    let client = ctx.daemon.client().await.unwrap();
    let info = client
        .call::<rail_core::ops::op::DaemonInfoOp>(&rail_core::ops::DaemonInfoRequest {})
        .await
        .unwrap();
    assert_eq!(
        info.pending, 0,
        "the quit cancelled the login in the daemon"
    );
}

/// Run every spawned job to its end and feed what it reports back to the screen, until
/// nothing is left to run. Returns every event the screen received.
async fn settle_effects(screen: &mut WizardScreen, mut effects: Vec<Effect>) -> Vec<Event> {
    let mut seen = Vec::new();
    while let Some(effect) = effects.pop() {
        let Effect::Spawn(job) = effect else {
            continue;
        };
        let (tx, mut results) = mpsc::unbounded_channel();
        job(TaskSink::new(tx, 0)).await;
        while let Ok((_, result)) = results.try_recv() {
            if let TaskResult::Wizard(event) = &result {
                seen.push(event.clone());
            }
            effects.extend(screen.update(Msg::Task(result)));
        }
    }
    seen
}

#[tokio::test]
async fn a_completed_login_leaves_no_token_or_code_in_the_tui() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), "[general]\nsecrets = \"file\"\n").unwrap();
    let server = token_server("ACCESS-only-in-the-daemon", "REFRESH-only-in-the-daemon").await;
    let (ctx, urls) = daemon_ctx(&paths, Some(format!("{}/token", server.uri()))).await;
    let config = rail_core::config::load(&paths.config_file()).unwrap();
    let mut screen = WizardScreen::new(
        Wizard::new(Some(config), None),
        ctx.clone(),
        crate::tui::test_util::keymap(),
    );
    let browser = redirect_when_asked(urls);
    let effects = drive(&mut screen, gmail_own_app_keys("me@example.com"));
    let events = settle_effects(&mut screen, effects).await;
    browser.await.unwrap();
    // A first run: the Keys step comes first, and Rail's own keys need nothing saved.
    assert_eq!(screen.wizard.step(), Step::Keys, "{events:?}");
    let effects = drive(&mut screen, vec![key(KeyCode::Enter)]);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(screen.wizard.step(), Step::Done);
    assert_eq!(screen.added(), ["me"]);
    let mut texts = vec![format!("{events:?}"), format!("{:?}", screen.wizard)];
    for event in &events {
        if let Event::LoginFinished(Ok(tokens)) = event {
            texts.push(tokens.access_token.clone());
            texts.push(tokens.refresh_token.clone());
        }
    }
    for text in &texts {
        for secret in ["ACCESS-only", "REFRESH-only", "c0de"] {
            assert!(!text.contains(secret), "{secret} reached the TUI: {text}");
        }
    }
    let stored = FileSecretStore::new(paths.secrets_file());
    let tokens = rail_core::auth::SecretStore::get_tokens(&stored, "me@example.com")
        .unwrap()
        .unwrap();
    assert_eq!(tokens.access_token, "ACCESS-only-in-the-daemon");
}

/// A rebound wizard key works, the default it replaced does nothing, and the hint says so.
#[test]
fn a_rebound_wizard_key_moves_the_list_and_the_hint_follows() {
    let text = "[keys]\n\"wizard.down\" = \"s\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    let keymap =
        Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap());
    let mut screen = WizardScreen::new(
        Wizard::new(Some(crate::tui::test_util::existing_config()), None),
        crate::tui::test_util::test_ctx(),
        keymap,
    );
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Preset, "↓ no longer moves");
    let mut screen = WizardScreen::new(
        Wizard::new(Some(crate::tui::test_util::existing_config()), None),
        crate::tui::test_util::test_ctx(),
        screen.keymap.clone(),
    );
    press(&mut screen, KeyCode::Char('s'));
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Provider);
    let text = crate::tui::test_util::snapshot(80, 24, |frame| {
        screen.view(frame, &crate::tui::theme::Theme::default())
    });
    assert!(text.contains("↑/s move"), "{text}");
}

#[test]
fn imapsmtp_ui_walk_and_validation() {
    let mut screen = wizard_screen();
    // Choose Set up my own app
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Provider);

    // Down twice to Generic IMAP / SMTP
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Instructions);
    assert_eq!(
        screen.wizard.draft().kind,
        Some(rail_core::config::AccountKind::ImapSmtp)
    );

    // Enter to Details
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.wizard.step(), Step::Details);

    // Fill fields
    type_text(&mut screen, "user@example.com");
    press(&mut screen, KeyCode::Tab); // to Name
    press(&mut screen, KeyCode::Tab); // to Color
    press(&mut screen, KeyCode::Tab); // to ImapHost
    type_text(&mut screen, "imap.example.com");
    press(&mut screen, KeyCode::Tab); // to ImapPort (already 993)
    press(&mut screen, KeyCode::Tab); // to Password
    type_text(&mut screen, "secret");
    press(&mut screen, KeyCode::Tab); // to SmtpHost
    type_text(&mut screen, "smtp.example.com");

    // Submit details
    let effects = screen.update(Msg::Key(key(KeyCode::Enter)));
    assert_eq!(screen.wizard.step(), Step::Login);
    assert!(screen.wizard.busy().is_some());
    assert!(!effects.is_empty());

    // Validate feedback
    feed(&mut screen, Event::ImapSmtpValidated(Ok(())));
    assert_eq!(screen.wizard.busy(), Some(Busy::Persisting));

    // Persisted feedback
    feed(&mut screen, Event::Persisted(Ok(report())));
    assert!(matches!(screen.wizard.step(), Step::Keys | Step::Done));
}
