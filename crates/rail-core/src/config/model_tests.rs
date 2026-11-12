use super::model::*;
use std::time::Duration;

fn with_account(general: &str, account: &str) -> crate::config::Config {
    let text = format!(
        "[general]\n{general}\n[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{account}"
    );
    crate::config::parse(&text, std::path::Path::new("c.toml")).unwrap()
}

/// AC-34: `attachment_dir` reaches `General` exactly as it was typed — the client expands
/// `~` and absolutizes, not the parser — and an unknown key still refuses the file.
#[test]
fn attachment_dir_is_read_verbatim() {
    let config = with_account("attachment_dir = \"~/mail\"", "");
    assert_eq!(
        config.general.attachment_dir,
        Some(std::path::PathBuf::from("~/mail"))
    );
    assert_eq!(with_account("", "").general.attachment_dir, None);
    assert_eq!(General::default().attachment_dir, None);
    let text = "[general]\nattachement_dir = \"~/mail\"\n";
    assert!(crate::config::parse(text, std::path::Path::new("c.toml")).is_err());
}

#[test]
fn each_kind_has_its_folder_model_and_its_move_kinds() {
    use crate::provider::FolderKind;
    assert_eq!(AccountKind::Gmail.folder_model(), FolderModel::Labels);
    assert_eq!(AccountKind::M365.folder_model(), FolderModel::SingleFolder);
    assert_eq!(
        AccountKind::ImapSmtp.folder_model(),
        FolderModel::SingleFolder
    );
    assert_eq!(
        FolderModel::Labels.move_kinds(),
        [FolderKind::Label, FolderKind::User]
    );
    assert_eq!(
        FolderModel::SingleFolder.move_kinds(),
        [
            FolderKind::Inbox,
            FolderKind::Archive,
            FolderKind::Spam,
            FolderKind::User
        ]
    );
}

#[test]
fn sync_settings_apply_overrides_and_the_clamp() {
    let cases = [
        ("", "", (0, 30)),
        ("history_days = 2", "", (2, 2)),
        ("history_days = 10\nbody_cache_days = 5", "", (10, 5)),
        ("history_days = 10", "history_days = 0", (0, 30)),
        ("", "history_days = 7\nbody_cache_days = 9", (7, 7)),
        ("body_cache_days = 3", "body_cache_days = 40", (0, 40)),
    ];
    for (general, account, (history, body)) in cases {
        let config = with_account(general, account);
        let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
        assert_eq!(
            (settings.history_days, settings.body_cache_days),
            (history, body),
            "{general:?} / {account:?}"
        );
        assert_eq!(settings.poll_interval, Duration::from_secs(60));
    }
}

#[test]
fn the_window_starts_history_days_ago_in_milliseconds() {
    let config = with_account("history_days = 2", "");
    let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
    let now = 10 * DAY_SECS;
    assert_eq!(settings.window_start_ms(now), Some(8 * DAY_SECS * 1000));
    assert_eq!(settings.body_since_ms(now), 8 * DAY_SECS * 1000);
    let whole = with_account("", "");
    let whole = SyncSettings::for_account(&whole.general, &whole.accounts[0]);
    assert_eq!(whole.window_start_ms(now), None);
}

/// Review minor 7: a window reaching before 1970 is the whole mailbox, never a negative
/// `after:`.
#[test]
fn a_window_before_1970_is_the_whole_mailbox() {
    let config = with_account(&format!("history_days = {}", u32::MAX), "");
    let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
    let now = 20_000 * DAY_SECS;
    assert_eq!(settings.window_start_ms(now), None);
    let config = with_account("history_days = 20000", "");
    let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
    assert_eq!(
        settings.window_start_ms(now),
        Some(0),
        "exactly 1970 is kept"
    );
    let config = with_account("history_days = 20001", "");
    let settings = SyncSettings::for_account(&config.general, &config.accounts[0]);
    assert_eq!(settings.window_start_ms(now), None);
}

#[test]
fn account_overrides_debug_print() {
    let config = with_account("", "history_days = 3");
    let debug = format!("{:?}", config.accounts[0]);
    assert!(debug.contains("history_days: Some(3)"), "{debug}");
}

/// Parked item 3: bidi overrides and zero-width characters are Unicode format (Cf)
/// characters; an address holding one could display as another.
#[test]
fn format_characters_are_not_plain() {
    for c in [
        '\u{202E}',
        '\u{200B}',
        '\u{200D}',
        '\u{2066}',
        '\u{FEFF}',
        '\u{E0001}',
    ] {
        assert!(!is_plain(&format!("me{c}@example.com")), "{:?}", c);
    }
    assert!(is_plain("ünïcödé@example.com"));
}

#[test]
fn account_debug_hides_the_client_secret() {
    let account = AccountConfig {
        client_secret: Some("GOCSPX-very-secret".into()),
        ..crate::test_support::sample_account()
    };
    let debug = format!("{account:?}");
    assert!(!debug.contains("GOCSPX-very-secret"), "{debug}");
    assert!(debug.contains("<redacted>"), "{debug}");
    assert!(debug.contains("me@example.com"), "{debug}");
}

#[test]
fn ai_send_defaults_to_confirm_and_parses_each_value() {
    use crate::config::parse;
    use std::path::Path;
    let account = |extra: &str| {
        format!(
            "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{extra}"
        )
    };
    let unset = parse(&account(""), Path::new("c.toml")).unwrap();
    assert_eq!(unset.accounts[0].ai_send, None);
    assert_eq!(unset.accounts[0].ai_send(), AiSend::Confirm);
    for (text, expected) in [
        ("confirm", AiSend::Confirm),
        ("drafts", AiSend::Drafts),
        ("client", AiSend::Client),
    ] {
        let line = format!("ai_send = \"{text}\"\n");
        let config = parse(&account(&line), Path::new("c.toml")).unwrap();
        assert_eq!(config.accounts[0].ai_send(), expected);
        assert_eq!(expected.as_str(), text);
    }
}

#[test]
fn from_name_loads_with_spaces_and_refuses_controls_and_length() {
    let config = with_account("", "from_name = \"Dana Reyes\"");
    assert_eq!(config.accounts[0].from_name.as_deref(), Some("Dana Reyes"));
    assert_eq!(with_account("", "").accounts[0].from_name, None);
    assert_eq!(
        with_account("", "from_name = \"\"").accounts[0].from_name,
        None
    );
    let path = std::path::Path::new("c.toml");
    let account = |line: &str| {
        format!(
            "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\n{line}\n"
        )
    };
    for bad in [
        "from_name = \"Dana\\tReyes\"",
        "from_name = \"Dana\\u202EReyes\"",
    ] {
        let err = crate::config::parse(&account(bad), path).unwrap_err();
        assert!(err.to_string().contains("from_name"), "{err}");
    }
    let long = format!("from_name = \"{}\"", "a".repeat(MAX_FROM_NAME_CHARS + 1));
    let err = crate::config::parse(&account(&long), path).unwrap_err();
    assert!(err.to_string().contains("at most 200"), "{err}");
    let fits = format!("from_name = \"{}\"", "a".repeat(MAX_FROM_NAME_CHARS));
    assert!(crate::config::parse(&account(&fits), path).is_ok());
}

#[test]
fn an_unknown_ai_send_value_reports_its_line() {
    use crate::config::{ConfigError, parse};
    use std::path::Path;
    let text = "[[accounts]]\nname = \"w\"\nkind = \"gmail\"\naddress = \"w@example.com\"\nclient_id = \"c\"\nai_send = \"always\"\n";
    let err = parse(text, Path::new("/x/config.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { line: 6, .. }), "{err}");
    assert!(err.to_string().contains("always"), "{err}");
}

#[test]
fn the_trash_prompt_names_each_providers_trash() {
    assert_eq!(
        AccountKind::Gmail.trash_note(),
        "Gmail keeps trashed mail in its Trash for 30 days."
    );
    assert_eq!(AccountKind::M365.trash_note(), "Moves to Deleted Items.");
    assert_eq!(AccountKind::ImapSmtp.trash_note(), "Moves to Trash.");
}

#[test]
fn each_kind_names_its_drafts_folder() {
    assert_eq!(AccountKind::Gmail.drafts_name(), "Gmail's Drafts");
    assert_eq!(AccountKind::M365.drafts_name(), "Outlook's Drafts");
    assert_eq!(AccountKind::ImapSmtp.drafts_name(), "Drafts");
}

#[test]
fn imap_smtp_kind_parses_and_reports_provider_name() {
    assert_eq!(AccountKind::ImapSmtp.provider_name(), "IMAP/SMTP");
    assert_eq!(AccountKind::ImapSmtp.as_str(), "imapsmtp");
    assert_eq!(AccountKind::parse("imapsmtp"), Some(AccountKind::ImapSmtp));
}

#[test]
fn tls_mode_strings_roundtrip() {
    assert_eq!(TlsMode::Required.as_str(), "required");
    assert_eq!(TlsMode::Opportunistic.as_str(), "opportunistic");
    assert_eq!(TlsMode::Off.as_str(), "off");
}
