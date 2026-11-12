//! `secrets.configure`: switching, moving, refusing, and the confirmation text and plan.
use super::*;

/// `switch_backend` as a front end calls it after the person said yes: with the plan
/// digest `describe_switch` gives for the request, when it describes one.
fn switch(
    paths: &Paths,
    ids: &ItemIdCache,
    request: &SecretsConfigureRequest,
) -> Result<SecretsConfigureResponse, OpError> {
    let mut request = request.clone();
    if request.confirm
        && let Ok(summary) = describe_switch(paths, ids, &request)
    {
        request.plan = Some(summary.plan());
    }
    switch_backend(paths, ids, &request)
}
use crate::auth::{FileSecretStore, OnePasswordSecretStore, TokenSet};
use crate::config::load;
use crate::ops::Secret;
use crate::test_support::FakeOp;

fn tokens(n: i64) -> TokenSet {
    TokenSet {
        access_token: format!("access-{n}"),
        refresh_token: format!("refresh-{n}"),
        expires_at: n,
    }
}

const TWO_ACCOUNTS: &str = "# mine\n[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"a\"\nkind = \"gmail\"\naddress = \"a@x.com\"\nclient_id = \"c\"\n\n[[accounts]]\nname = \"b\"\nkind = \"gmail\"\naddress = \"b@x.com\"\nclient_id = \"c\"\n";

/// Two accounts on the file backend, with `op_path` set by hand to the fake, as a user
/// would. Returns the config text as written.
fn setup(tmp: &tempfile::TempDir) -> (Paths, FakeOp, FileSecretStore, String) {
    let paths = Paths::all_in(tmp.path());
    let fake = FakeOp::install(tmp.path());
    let text = format!("{TWO_ACCOUNTS}{}", fake.config_toml());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), &text).unwrap();
    let file = FileSecretStore::new(paths.secrets_file());
    file.set_tokens("a@x.com", &tokens(1)).unwrap();
    file.set_tokens("b@x.com", &tokens(2)).unwrap();
    (paths, fake, file, text)
}

fn to_onepassword() -> SecretsConfigureRequest {
    SecretsConfigureRequest {
        backend: SecretsBackend::OnePassword,
        vault: Some("Rail".into()),
        op_account: None,
        migrate: true,
        confirm: true,
        plan: None,
    }
}

#[test]
fn switching_to_1password_moves_tokens_and_deletes_the_old_copies() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    let done = switch(&paths, &ids, &to_onepassword()).unwrap();
    assert_eq!(done.previous, SecretsBackend::File);
    assert_eq!(done.moved, ["a@x.com", "b@x.com"]);
    assert!(done.changed);
    assert!(done.old_copies_kept.is_empty());
    assert_eq!(file.get_tokens("a@x.com").unwrap(), None);
    let config = load(&paths.config_file()).unwrap();
    assert_eq!(config.general.secrets, SecretsBackend::OnePassword);
    assert_eq!(config.onepassword.as_ref().unwrap().vault, "Rail");
    assert!(
        std::fs::read_to_string(paths.config_file())
            .unwrap()
            .contains("# mine")
    );
    let onepassword = OnePasswordSecretStore::new(fake.cli(), "Rail".into(), ids);
    assert_eq!(
        config.onepassword.as_ref().unwrap().op_path.as_deref(),
        Some(fake.path.as_path()),
        "the hand-set op_path is kept"
    );
    assert_eq!(onepassword.get_tokens("b@x.com").unwrap(), Some(tokens(2)));
}

#[test]
fn switching_to_1password_moves_passwords_and_deletes_old_copies() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    file.set_password("a@x.com", &Secret::new("pass-a"))
        .unwrap();
    file.set_password("a@x.com:smtp", &Secret::new("smtp-a"))
        .unwrap();

    let done = switch(&paths, &ids, &to_onepassword()).unwrap();
    assert_eq!(done.previous, SecretsBackend::File);
    assert_eq!(done.moved, ["a@x.com", "b@x.com"]);
    assert!(done.changed);
    assert!(done.old_copies_kept.is_empty());
    assert_eq!(file.get_password("a@x.com").unwrap(), None);
    assert_eq!(file.get_password("a@x.com:smtp").unwrap(), None);

    let onepassword = OnePasswordSecretStore::new(fake.cli(), "Rail".into(), ids);
    assert_eq!(
        onepassword
            .get_password("a@x.com")
            .unwrap()
            .as_ref()
            .map(|s| s.expose()),
        Some("pass-a")
    );
    assert_eq!(
        onepassword
            .get_password("a@x.com:smtp")
            .unwrap()
            .as_ref()
            .map(|s| s.expose()),
        Some("smtp-a")
    );
}

#[test]
fn a_failure_part_way_leaves_the_old_copies_and_the_config() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, text) = setup(&tmp);
    fake.fail_on("b@x.com");
    let err = switch(&paths, &ItemIdCache::default(), &to_onepassword()).unwrap_err();
    assert_eq!(err.code, ErrorCode::Secrets);
    assert!(!err.message.contains("access-2"), "{}", err.message);
    assert!(
        err.message
            .contains("Copied before the failure (old copies kept): [a@x.com]"),
        "{}",
        err.message
    );
    assert!(
        err.message.contains("not copied: [b@x.com]"),
        "{}",
        err.message
    );
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    assert_eq!(file.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
    assert_eq!(file.get_tokens("b@x.com").unwrap(), Some(tokens(2)));
}

#[test]
fn a_locked_1password_stops_the_switch_before_anything_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, text) = setup(&tmp);
    fake.lock();
    let err = switch(&paths, &ItemIdCache::default(), &to_onepassword()).unwrap_err();
    assert!(err.hint.unwrap().contains("op signin"));
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    assert_eq!(file.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
}

/// Review focus 6: no request field reaches `op_path`.
#[test]
fn a_request_cannot_choose_the_program_rail_runs() {
    let schema = serde_json::to_string(&schemars::schema_for!(SecretsConfigureRequest)).unwrap();
    assert!(!schema.contains("op_path"), "{schema}");
    let with_path =
        serde_json::json!({"backend": "1password", "vault": "Rail", "op_path": "/tmp/evil"});
    let request: SecretsConfigureRequest = serde_json::from_value(with_path).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), TWO_ACCOUNTS).unwrap();
    let _ = switch(&paths, &ItemIdCache::default(), &request);
    let written = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(!written.contains("/tmp/evil"), "{written}");
}

#[test]
fn switching_without_migrate_only_changes_the_config() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, _text) = setup(&tmp);
    let request = SecretsConfigureRequest {
        migrate: false,
        ..to_onepassword()
    };
    let done = switch(&paths, &ItemIdCache::default(), &request).unwrap();
    assert!(done.moved.is_empty());
    assert_eq!(done.old_copies_kept, ["a@x.com", "b@x.com"]);
    assert_eq!(file.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
    assert!(fake.items().is_empty());
}

#[test]
fn the_same_backend_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, text) = setup(&tmp);
    let request = SecretsConfigureRequest {
        backend: SecretsBackend::File,
        vault: None,
        op_account: None,
        migrate: true,
        confirm: false,
        plan: None,
    };
    let done = switch(&paths, &ItemIdCache::default(), &request).unwrap();
    assert!(done.moved.is_empty());
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    assert!(!done.changed);
}

#[test]
fn onepassword_without_a_vault_is_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), TWO_ACCOUNTS).unwrap();
    let request = SecretsConfigureRequest {
        backend: SecretsBackend::OnePassword,
        vault: None,
        op_account: None,
        migrate: true,
        confirm: true,
        plan: None,
    };
    let err = switch(&paths, &ItemIdCache::default(), &request).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.hint.unwrap().contains("vault"));
}

#[test]
fn an_unconfirmed_change_is_refused_with_nothing_written() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, text) = setup(&tmp);
    let request = SecretsConfigureRequest {
        confirm: false,
        ..to_onepassword()
    };
    let err = switch(&paths, &ItemIdCache::default(), &request).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotConfirmed);
    assert_eq!(err.class, ErrorClass::Permanent);
    assert_eq!(err.message, NEEDS_CONFIRMATION);
    let confirmation = err.confirmation.unwrap();
    assert!(confirmation.text.contains("2 account(s)"));
    assert_eq!(confirmation.plan.len(), 64);
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    assert_eq!(file.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
    assert!(fake.items().is_empty());
}

/// What the person confirmed is what runs. An account whose tokens appear
/// between the question and the answer changes the plan, so the answer no longer counts.
#[test]
fn a_plan_that_changed_after_the_question_is_refused_with_the_fresh_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    let asked = describe_switch(&paths, &ids, &to_onepassword()).unwrap();
    let third = "\n[[accounts]]\nname = \"c\"\nkind = \"gmail\"\naddress = \"c@x.com\"\nclient_id = \"c\"\n";
    let mut config = std::fs::read_to_string(paths.config_file()).unwrap();
    config.push_str(third);
    std::fs::write(paths.config_file(), &config).unwrap();
    file.set_tokens("c@x.com", &tokens(3)).unwrap();
    let stale = SecretsConfigureRequest {
        plan: Some(asked.plan()),
        ..to_onepassword()
    };
    let err = switch_backend(&paths, &ids, &stale).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotConfirmed);
    let fresh = err.confirmation.unwrap();
    assert_ne!(fresh.plan, asked.plan());
    assert!(fresh.text.contains("3 account(s)"), "{}", fresh.text);
    assert_eq!(
        std::fs::read_to_string(paths.config_file()).unwrap(),
        config
    );
    assert_eq!(file.get_tokens("c@x.com").unwrap(), Some(tokens(3)));
    assert!(fake.items().is_empty());
}

#[test]
fn a_confirmed_change_without_a_plan_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, _file, text) = setup(&tmp);
    let err = switch_backend(&paths, &ItemIdCache::default(), &to_onepassword()).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotConfirmed);
    assert!(err.confirmation.is_some());
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    assert!(fake.items().is_empty());
}

/// A vault name cannot forge part of the text or hide characters in it.
#[test]
fn a_vault_name_cannot_forge_or_hide_text_in_the_confirmation() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    let vault = "Rail\"). Tokens for 0 account(s) will be moved.";
    let request = SecretsConfigureRequest {
        vault: Some(vault.into()),
        ..to_onepassword()
    };
    let text = describe_switch(&paths, &ids, &request).unwrap().text();
    assert!(
        text.contains(r#"(vault "Rail\"). Tokens for 0 account(s) will be moved.")"#),
        "{text}"
    );
    assert!(
        text.contains(". Tokens for 2 account(s) will be moved."),
        "{text}"
    );
    // A bidi override is refused before any text is built (parked item 3).
    let request = SecretsConfigureRequest {
        vault: Some("Rail\u{202e}lanosrep".into()),
        ..to_onepassword()
    };
    let err = describe_switch(&paths, &ids, &request).unwrap_err();
    assert!(
        err.message.contains("control or format characters"),
        "{err}"
    );
}

#[test]
fn a_switch_without_migrate_says_each_account_needs_a_login() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, _text) = setup(&tmp);
    let request = SecretsConfigureRequest {
        migrate: false,
        ..to_onepassword()
    };
    let summary = describe_switch(&paths, &ItemIdCache::default(), &request).unwrap();
    let text = summary.text();
    assert!(text.contains("Tokens stay where they are"), "{text}");
    assert!(text.contains("rail account login <name>"), "{text}");
    assert!(summary.moving.is_empty());
}

#[test]
fn the_summary_names_the_target_and_warns_about_the_plain_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    let summary = describe_switch(&paths, &ids, &to_onepassword()).unwrap();
    assert!(summary.changes);
    assert_eq!(summary.moving, ["a@x.com", "b@x.com"]);
    let text = summary.text();
    assert!(text.contains("to 1password (vault \"Rail\")"), "{text}");
    assert!(text.contains("2 account(s)"), "{text}");
    assert!(!text.contains(FILE_WARNING));
    switch(&paths, &ids, &to_onepassword()).unwrap();
    let back = SecretsConfigureRequest {
        backend: SecretsBackend::File,
        vault: None,
        op_account: None,
        migrate: true,
        confirm: true,
        plan: None,
    };
    let text = describe_switch(&paths, &ids, &back).unwrap().text();
    assert!(text.ends_with(FILE_WARNING), "{text}");
}

#[test]
fn changing_only_the_vault_moves_the_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake, _file, _text) = setup(&tmp);
    let ids = ItemIdCache::default();
    switch(&paths, &ids, &to_onepassword()).unwrap();
    let other = SecretsConfigureRequest {
        vault: Some("Other".into()),
        ..to_onepassword()
    };
    let done = switch(&paths, &ids, &other).unwrap();
    assert_eq!(done.moved, ["a@x.com", "b@x.com"]);
    assert!(done.old_copies_kept.is_empty());
    let config = load(&paths.config_file()).unwrap();
    assert_eq!(config.onepassword.as_ref().unwrap().vault, "Other");
    let store = OnePasswordSecretStore::new(fake.cli(), "Other".into(), ids.clone());
    assert_eq!(store.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
    let old = OnePasswordSecretStore::new(fake.cli(), "Rail".into(), ids);
    assert_eq!(old.get_tokens("a@x.com").unwrap(), None);
}

#[test]
fn bad_vault_and_account_values_are_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, text) = setup(&tmp);
    for request in [
        SecretsConfigureRequest {
            vault: Some("a/b".into()),
            ..to_onepassword()
        },
        SecretsConfigureRequest {
            op_account: Some("  ".into()),
            ..to_onepassword()
        },
    ] {
        let err = switch(&paths, &ItemIdCache::default(), &request).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{err:?}");
    }
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
}

fn account_cli(fake: &FakeOp, account: &str) -> crate::auth::OpCli {
    let config = OnePasswordConfig {
        account: Some(account.into()),
        ..fake.config()
    };
    crate::auth::OpCli::from_config(Some(&config), &fake.runtime_dir)
}

/// Both accounts hold tokens in vault "Rail" of 1Password account `account`.
fn on_1password(tmp: &tempfile::TempDir, account: Option<&str>) -> (Paths, FakeOp) {
    let paths = Paths::all_in(tmp.path());
    let fake = FakeOp::install(tmp.path());
    let account_line = account.map_or(String::new(), |a| format!("account = \"{a}\"\n"));
    let table = fake.config_toml().replace(
        "vault = \"Rail\"\n",
        &format!("vault = \"Rail\"\n{account_line}"),
    );
    let text = format!(
        "{}{table}",
        TWO_ACCOUNTS.replace("secrets = \"file\"", "secrets = \"1password\"")
    );
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), text).unwrap();
    let cli = match account {
        Some(a) => account_cli(&fake, a),
        None => fake.cli(),
    };
    let store = OnePasswordSecretStore::new(cli, "Rail".into(), ItemIdCache::default());
    store.set_tokens("a@x.com", &tokens(1)).unwrap();
    store.set_tokens("b@x.com", &tokens(2)).unwrap();
    (paths, fake)
}

fn same_backend_other_account(account: &str, vault: Option<&str>) -> SecretsConfigureRequest {
    SecretsConfigureRequest {
        vault: vault.map(Into::into),
        op_account: Some(account.into()),
        ..to_onepassword()
    }
}

#[test]
fn another_account_with_the_same_vault_name_gets_the_tokens_and_loses_the_old_copies() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake) = on_1password(&tmp, Some("A"));
    let ids = ItemIdCache::default();
    let request = same_backend_other_account("B", None);
    let done = switch(&paths, &ids, &request).unwrap();
    assert_eq!(done.moved, ["a@x.com", "b@x.com"]);
    assert!(done.old_copies_kept.is_empty(), "{done:?}");
    assert!(done.already_in_place.is_empty());
    assert_eq!(fake.item_count("A", "rail"), 0, "old copies deleted");
    assert_eq!(fake.item_count("B", "rail"), 2, "new copies written");
    let new = OnePasswordSecretStore::new(account_cli(&fake, "B"), "Rail".into(), ids);
    assert_eq!(new.get_tokens("b@x.com").unwrap(), Some(tokens(2)));
}

#[test]
fn one_store_named_two_ways_deletes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake) = on_1password(&tmp, None);
    let ids = ItemIdCache::default();
    let request = same_backend_other_account("main", Some("rail"));
    let done = switch(&paths, &ids, &request).unwrap();
    assert!(done.moved.is_empty(), "{done:?}");
    assert_eq!(done.already_in_place, ["a@x.com", "b@x.com"]);
    assert!(done.old_copies_kept.is_empty());
    assert_eq!(fake.items().len(), 2, "the only copies survive");
    let config = load(&paths.config_file()).unwrap();
    let store = OnePasswordSecretStore::new(fake.cli(), config.onepassword.unwrap().vault, ids);
    assert_eq!(store.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
}

#[test]
fn an_unverifiable_new_copy_keeps_the_old_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, fake) = on_1password(&tmp, Some("A"));
    // Account B's third `item list` on fails: after both copies, before they are checked.
    fake.fail_list_from("B", 3);
    let ids = ItemIdCache::default();
    let done = switch(&paths, &ids, &same_backend_other_account("B", None)).unwrap();
    assert_eq!(done.old_copies_kept, ["a@x.com", "b@x.com"], "{done:?}");
    assert!(done.already_in_place.is_empty());
    assert_eq!(fake.item_count("A", "rail"), 2, "nothing was deleted");
    let old = OnePasswordSecretStore::new(account_cli(&fake, "A"), "Rail".into(), ids);
    assert_eq!(old.get_tokens("a@x.com").unwrap(), Some(tokens(1)));
}

/// Names longer than the limit are refused at input, without echoing them.
#[test]
fn a_vault_or_account_over_the_limit_is_refused_without_echo() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, text) = setup(&tmp);
    let long = "V".repeat(crate::ops::MAX_NAME_CHARS + 1);
    for (field, request) in [
        (
            "vault",
            SecretsConfigureRequest {
                vault: Some(long.clone()),
                ..to_onepassword()
            },
        ),
        (
            "op_account",
            SecretsConfigureRequest {
                op_account: Some(long.clone()),
                ..to_onepassword()
            },
        ),
    ] {
        let err = switch_backend(&paths, &ItemIdCache::default(), &request).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{err:?}");
        assert!(err.message.contains(field), "{err:?}");
        assert!(!format!("{err:?}").contains(&long), "{err:?}");
    }
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
}

/// Parked item 2: the daemon refuses what MCP used to refuse alone, so `rail secrets set`
/// and `rail_secrets_configure` check the same way.
#[test]
fn a_vault_or_account_with_control_or_format_characters_is_refused_without_echo() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake, _file, text) = setup(&tmp);
    for (field, request) in [
        (
            "vault",
            SecretsConfigureRequest {
                vault: Some("Rail\"). Tokens for 0 account(s) will be moved.\n".into()),
                ..to_onepassword()
            },
        ),
        (
            "vault",
            SecretsConfigureRequest {
                vault: Some("Ra\u{202E}il".into()),
                ..to_onepassword()
            },
        ),
        (
            "op_account",
            SecretsConfigureRequest {
                op_account: Some("me\rx".into()),
                ..to_onepassword()
            },
        ),
    ] {
        let err = switch_backend(&paths, &ItemIdCache::default(), &request).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{err:?}");
        assert!(err.message.starts_with(field), "{err}");
        assert!(
            err.message.contains("control or format characters"),
            "{err}"
        );
        assert!(!err.message.contains("Tokens for"), "{err}");
        assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), text);
    }
}

/// The old vault and 1Password account decide where tokens are read and deleted, so a hand
/// edit of either between the question and the answer changes the plan; the text names them.
#[test]
fn a_hand_edit_of_the_old_store_changes_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let (paths, _fake) = on_1password(&tmp, Some("A"));
    let ids = ItemIdCache::default();
    let to_file = SecretsConfigureRequest {
        backend: SecretsBackend::File,
        vault: None,
        op_account: None,
        migrate: false,
        confirm: false,
        plan: None,
    };
    let asked = describe_switch(&paths, &ids, &to_file).unwrap();
    let text = asked.text();
    assert!(
        text.contains("from 1password (vault \"Rail\", account \"A\") to file"),
        "{text}"
    );
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    for edited in [
        config.replace("account = \"A\"", "account = \"B\""),
        config.replace("vault = \"Rail\"", "vault = \"Other\""),
    ] {
        std::fs::write(paths.config_file(), &edited).unwrap();
        let now = describe_switch(&paths, &ids, &to_file).unwrap();
        assert!(now.moving.is_empty() && asked.moving.is_empty());
        assert_ne!(now.plan(), asked.plan(), "{edited}");
    }
}
