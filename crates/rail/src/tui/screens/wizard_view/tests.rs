use crossterm::event::KeyCode;
use rail_core::config::AccountKind;
use rail_core::error::ErrorClass;
use rail_core::onboarding::{Event, Failure, Preset, PresetEntry};

use url::Url;

use crate::tui::app::{Effect, Msg, TaskResult};
use crate::tui::screen::Screen;
use crate::tui::screens::wizard::WizardScreen;
use crate::tui::test_util::{
    SIZES, feed, gmail_own_app_keys, press, render_lines, snapshot, type_text, wizard_screen,
};
use crate::tui::theme::Theme;
use crate::tui::widgets::status_line::SPINNER;

fn draw(screen: &WizardScreen, width: u16, height: u16) -> String {
    render_lines(width, height, |frame| screen.view(frame, &Theme::default())).join("\n")
}

fn keys(screen: &mut WizardScreen, codes: &[KeyCode]) {
    for code in codes {
        press(screen, *code);
    }
}

fn to_gmail_details(screen: &mut WizardScreen) {
    keys(
        screen,
        &[
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Enter,
        ],
    );
}

/// The wizard's steps as the keymap migration must leave them, at both sizes. Taken before
/// the migration; the migration may not change a single cell.
fn snapshot_states() -> Vec<(&'static str, WizardScreen)> {
    let mut provider = wizard_screen();
    keys(&mut provider, &[KeyCode::Down, KeyCode::Enter]);
    let mut instructions = wizard_screen();
    keys(
        &mut instructions,
        &[KeyCode::Down, KeyCode::Enter, KeyCode::Enter],
    );
    let mut details = wizard_screen();
    to_gmail_details(&mut details);
    let mut login = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        login.update(Msg::Key(k));
    }
    let url = Url::parse("https://accounts.example/auth?state=s").unwrap();
    feed(&mut login, Event::LoginUrl(url));
    let mut retry = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        retry.update(Msg::Key(k));
    }
    let failure = Failure::new("boom", ErrorClass::Transient);
    feed(&mut retry, Event::LoginFinished(Err(failure)));
    vec![
        ("start", wizard_screen()),
        ("provider", provider),
        ("instructions", instructions),
        ("details", details),
        ("login", login),
        ("retry", retry),
    ]
}

#[test]
fn wizard_snapshots() {
    for (name, screen) in snapshot_states() {
        for (width, height) in SIZES {
            let text = snapshot(width, height, |frame| screen.view(frame, &Theme::default()));
            insta::assert_snapshot!(format!("wizard_{name}_{width}x{height}"), text);
        }
    }
}

fn assert_shows(screen: &WizardScreen, needles: &[&str]) {
    for (width, height) in SIZES {
        let text = draw(screen, width, height);
        for needle in needles {
            assert!(
                text.contains(needle),
                "{needle:?} missing at {width}x{height}:\n{text}"
            );
        }
    }
}

/// Microsoft, own app, as an administrator, with the tenant left at `common`: the
/// wizard asks which organisation to approve Rail for.
fn to_consent_tenant(screen: &mut WizardScreen) {
    keys(
        screen,
        &[
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Enter,
        ],
    );
    type_text(screen, "me@contoso.com");
    keys(screen, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Tab]);
    type_text(screen, "cid");
    keys(screen, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Tab]);
    keys(screen, &[KeyCode::Char(' '), KeyCode::Enter]);
}

fn consent_required(screen: &mut WizardScreen) {
    keys(
        screen,
        &[
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Enter,
        ],
    );
    type_text(screen, "me@contoso.com");
    keys(screen, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Tab]);
    type_text(screen, "cid");
    press(screen, KeyCode::Enter);
    let failure = Failure {
        message: "AADSTS65001: consent required".into(),
        class: ErrorClass::Permanent,
        consent_required: true,
    };
    feed(screen, Event::LoginFinished(Err(failure)));
}

#[test]
fn start_shows_the_steps_and_both_choices_at_both_sizes() {
    let screen = wizard_screen();
    assert_shows(
        &screen,
        &[
            "Rail setup",
            "▸ Start",
            "Provider",
            "Admin consent",
            "Done",
            "Use a preset from my organisation",
            "Set up my own app",
            "Enter choose",
        ],
    );
}

#[test]
fn provider_shows_both_providers_at_both_sizes() {
    let mut screen = wizard_screen();
    keys(&mut screen, &[KeyCode::Down, KeyCode::Enter]);
    assert_shows(
        &screen,
        &[
            "Which provider hosts your mail?",
            "✓ Start",
            "▸ Provider",
            "Gmail / Google Workspace",
            "Microsoft 365 / Outlook",
            "↑/↓ move",
        ],
    );
}

#[test]
fn preset_shows_the_spinner_then_the_list_problems_and_source_at_both_sizes() {
    let mut screen = wizard_screen();
    press(&mut screen, KeyCode::Enter);
    let spinner = format!("{} Looking for presets", SPINNER[0]);
    assert_shows(&screen, &[spinner.as_str()]);
    let contoso = Preset {
        name: Some("Contoso Microsoft 365".into()),
        kind: AccountKind::M365,
        client_id: "contoso-app".into(),
        tenant: Some("contoso.onmicrosoft.com".into()),
        client_secret: None,
        help_url: None,
    };
    let entries = vec![
        PresetEntry {
            source: "/p/contoso.toml".into(),
            preset: Ok(contoso),
        },
        PresetEntry {
            source: "/p/bad.toml".into(),
            preset: Err("/p/bad.toml:2:1: unknown field".into()),
        },
    ];
    feed(&mut screen, Event::PresetsLoaded(entries));
    assert_shows(
        &screen,
        &[
            "Choose your organisation's preset",
            "▸ Start",
            "▸ Contoso Microsoft 365",
            "/p/bad.toml:2:1: unknown field",
            "Path or URL",
            "a preset file, or the https://",
        ],
    );
}

#[test]
fn instructions_scroll_at_both_sizes() {
    let mut screen = wizard_screen();
    keys(
        &mut screen,
        &[KeyCode::Down, KeyCode::Enter, KeyCode::Enter],
    );
    for (width, height) in SIZES {
        let text = draw(&screen, width, height);
        assert!(
            text.contains("1. In Google Cloud Console"),
            "{width}x{height}:\n{text}"
        );
        assert!(
            text.contains("o open the portal"),
            "{width}x{height}:\n{text}"
        );
    }
    // The panel remembers the last size drawn. On a short terminal the text overflows, and
    // End shows the last page of it, not a single row.
    draw(&screen, 80, 16);
    keys(&mut screen, &[KeyCode::End]);
    let text = draw(&screen, 80, 16);
    assert!(!text.contains("1. In Google Cloud Console"), "{text}");
    assert!(text.contains("gmail.modify"), "{text}");
    assert!(
        text.contains("on any port; there is nothing to add."),
        "{text}"
    );
    assert!(
        text.contains("4. Copy the client ID") || text.contains("step."),
        "{text}"
    );
    keys(&mut screen, &[KeyCode::Home]);
    let text = draw(&screen, 80, 16);
    assert!(text.contains("1. In Google Cloud Console"), "{text}");
    // Where everything fits there is nothing to scroll.
    draw(&screen, 120, 40);
    keys(&mut screen, &[KeyCode::Home, KeyCode::Down]);
    let text = draw(&screen, 120, 40);
    assert!(text.contains("1. In Google Cloud Console"), "{text}");
}

#[test]
fn details_show_labels_and_mask_the_secret_at_both_sizes() {
    let mut screen = wizard_screen();
    to_gmail_details(&mut screen);
    keys(
        &mut screen,
        &[KeyCode::Tab, KeyCode::Tab, KeyCode::Tab, KeyCode::Tab],
    );
    type_text(&mut screen, "hunter2");
    for (width, height) in SIZES {
        let text = draw(&screen, width, height);
        for needle in [
            "Email address",
            "Account name",
            "Colour",
            "Client ID",
            "Client secret",
            "•••••••",
            "✓ Provider",
        ] {
            assert!(
                text.contains(needle),
                "{needle:?} missing at {width}x{height}:\n{text}"
            );
        }
        assert!(
            !text.contains("hunter2"),
            "the secret leaked at {width}x{height}"
        );
    }
}

#[test]
fn details_show_the_error_line_under_the_field() {
    let mut screen = wizard_screen();
    to_gmail_details(&mut screen);
    type_text(&mut screen, "WORK@example.com");
    assert_shows(&screen, &["WORK@example.com is already configured"]);
}

#[test]
fn consent_tenant_asks_for_the_organisation_at_both_sizes() {
    let mut screen = wizard_screen();
    to_consent_tenant(&mut screen);
    assert_shows(
        &screen,
        &[
            "Which organisation are you approving Rail for?",
            "▸ Admin consent",
            "Org tenant",
            "contoso.com",
            "your tenant ID or domain",
        ],
    );
}

#[test]
fn admin_consent_explains_then_waits_with_the_spinner_and_url_at_both_sizes() {
    let mut screen = wizard_screen();
    to_consent_tenant(&mut screen);
    press(&mut screen, KeyCode::Enter);
    assert_shows(
        &screen,
        &[
            "Grant admin consent",
            "▸ Admin consent",
            "Rail needs an administrator",
        ],
    );
    let effects = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(effects.as_slice(), [Effect::Spawn(_)]),
        "{effects:?}"
    );
    let url = url::Url::parse("https://login.microsoftonline.com/c/adminconsent?state=s").unwrap();
    feed(&mut screen, Event::ConsentUrl(url));
    let spinner = format!("{} Waiting for admin consent", SPINNER[0]);
    assert_shows(
        &screen,
        &[
            "Approve Rail in your browser",
            "login.microsoftonline.com/c/adminconsent?state=s",
            spinner.as_str(),
        ],
    );
}

#[test]
fn login_shows_the_url_and_the_spinner_frame() {
    let mut screen = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        screen.update(Msg::Key(k));
    }
    let url = url::Url::parse("http://127.0.0.1:5555/auth?state=s").unwrap();
    feed(&mut screen, Event::LoginUrl(url));
    for _ in 0..3 {
        screen.update(Msg::Tick);
    }
    let spinner = format!("{} Waiting for the browser login", SPINNER[3]);
    assert_shows(
        &screen,
        &[
            "Log in as me@gmail.com",
            "http://127.0.0.1:5555/auth?state=s",
            spinner.as_str(),
            "▸ Login",
        ],
    );
}

#[test]
fn saving_shows_the_spinner_and_asks_to_wait() {
    let mut screen = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        screen.update(Msg::Key(k));
    }
    let tokens = rail_core::auth::TokenSet {
        access_token: "tok-access-value".into(),
        refresh_token: "tok-refresh-value".into(),
        expires_at: 1,
    };
    feed(&mut screen, Event::LoginFinished(Ok(tokens)));
    let spinner = format!("{} Saving… please wait", SPINNER[0]);
    assert_shows(&screen, &[spinner.as_str(), "▸ Login"]);
}

#[test]
fn a_consent_failure_shows_the_url_and_the_copy_hint() {
    let mut screen = wizard_screen();
    consent_required(&mut screen);
    assert_shows(
        &screen,
        &[
            "administrator must approve",
            "Press c to copy",
            "login.microsoftonline.com",
        ],
    );
    let copied = press(&mut screen, KeyCode::Char('c'));
    assert!(
        matches!(copied.as_slice(), [Effect::Copy(u)] if u.contains("adminconsent")),
        "{copied:?}"
    );
    let text = draw(&screen, 80, 24);
    assert!(
        !text.contains("Copied"),
        "no notice before the copy ran:\n{text}"
    );
}

/// The copy's outcome, not the key press, sets the notice.
#[test]
fn the_copy_outcome_sets_the_notice() {
    let mut screen = wizard_screen();
    consent_required(&mut screen);
    press(&mut screen, KeyCode::Char('c'));
    screen.update(Msg::Task(TaskResult::Copied(Ok(()))));
    assert_shows(
        &screen,
        &["Sent via OSC 52. If not copied, select the URL; it is also printed on quit"],
    );
    press(&mut screen, KeyCode::Char('c'));
    screen.update(Msg::Task(TaskResult::Copied(Err("no tty".into()))));
    assert_shows(&screen, &["Could not copy: no tty; the URL is shown above"]);
}

#[test]
fn the_login_url_comes_with_the_copy_hint() {
    let mut screen = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        screen.update(Msg::Key(k));
    }
    let url = url::Url::parse("https://accounts.example/auth?state=s").unwrap();
    screen.update(Msg::Task(TaskResult::Wizard(
        rail_core::onboarding::Event::LoginUrl(url),
    )));
    assert_shows(&screen, &["accounts.example/auth", "Press c to copy"]);
}

#[test]
fn a_panic_with_nothing_in_flight_still_shows_a_failure() {
    let mut screen = wizard_screen();
    screen.update(Msg::Task(TaskResult::JobPanicked("opener".into())));
    assert_shows(&screen, &["internal error: opener; see rail.log"]);
}

#[test]
fn done_shows_the_report_and_both_choices() {
    let mut screen = wizard_screen();
    for k in gmail_own_app_keys("me@gmail.com") {
        screen.update(Msg::Key(k));
    }
    let tokens = rail_core::auth::TokenSet {
        access_token: "tok-access-value".into(),
        refresh_token: "tok-refresh-value".into(),
        expires_at: 1,
    };
    feed(&mut screen, Event::LoginFinished(Ok(tokens)));
    let report = rail_core::onboarding::DoneReport {
        config_path: "/c/config.toml".into(),
        token_backend: "file".into(),
        token_location: "/d/secrets.json".into(),
        store_path: "/d/rail.db".into(),
    };
    feed(&mut screen, Event::Persisted(Ok(report)));
    for (width, height) in SIZES {
        let text = draw(&screen, width, height);
        for needle in [
            "All set",
            "/c/config.toml",
            "file (/d/secrets.json)",
            "/d/rail.db",
            "rail account login me",
            "Add another account",
            "Finish",
            "▸ Done",
        ] {
            assert!(
                text.contains(needle),
                "{needle:?} missing at {width}x{height}:\n{text}"
            );
        }
        assert!(
            !text.contains("tok-"),
            "token values never reach the screen:\n{text}"
        );
    }
}

/// A first run: after the account is saved, the Keys step lists every preset with what sets it
/// apart, under "Done" in the step list, and Enter on one starts saving it.
#[test]
fn a_first_run_shows_the_keys_step_and_enter_saves_the_preset() {
    let mut screen = WizardScreen::new(
        rail_core::onboarding::Wizard::new(None, None),
        crate::tui::test_util::test_ctx(),
        crate::tui::test_util::keymap(),
    );
    for k in gmail_own_app_keys("me@gmail.com") {
        screen.update(Msg::Key(k));
    }
    let tokens = rail_core::auth::TokenSet {
        access_token: "tok-access-value".into(),
        refresh_token: "tok-refresh-value".into(),
        expires_at: 1,
    };
    feed(&mut screen, Event::LoginFinished(Ok(tokens)));
    let report = rail_core::onboarding::DoneReport {
        config_path: "/c/config.toml".into(),
        token_backend: "file".into(),
        token_location: "/d/secrets.json".into(),
        store_path: "/d/rail.db".into(),
    };
    feed(&mut screen, Event::Persisted(Ok(report)));
    for (width, height) in SIZES {
        let text = draw(&screen, width, height);
        for needle in [
            "Choose your keys",
            "rail — Rail's own keys",
            "vim — h/l panes",
            "outlook — ",
            "▸ Done",
        ] {
            assert!(
                text.contains(needle),
                "{width}x{height} lacks {needle:?}:\n{text}"
            );
        }
    }
    press(&mut screen, KeyCode::Down);
    let effects = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(effects.as_slice(), [Effect::Spawn(_)]),
        "{effects:?}"
    );
    feed(&mut screen, Event::KeymapSaved(Ok(())));
    let text = draw(&screen, 100, 30);
    assert!(text.contains("All set") && text.contains("vim"), "{text}");
}
