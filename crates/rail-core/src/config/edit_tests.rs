use super::*;
use crate::config::{AccountKind, parse};
use std::path::Path;

fn account(name: &str, address: &str) -> AccountConfig {
    AccountConfig {
        name: name.into(),
        kind: AccountKind::M365,
        address: address.into(),
        color: Some("magenta".into()),
        label: None,
        tenant: Some("common".into()),
        ..crate::test_support::sample_account()
    }
}

const EXISTING: &str = "# my rail config\n[general]\neditor = \"nvim\" # the good one\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"g\"\n";

#[test]
fn append_preserves_comments_and_adds_a_table() {
    let out = append_account(
        EXISTING,
        Path::new("c.toml"),
        &account("corp", "me@corp.com"),
    )
    .unwrap();
    assert!(out.contains("# my rail config"));
    assert!(out.contains("# the good one"));
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.accounts.len(), 2);
    assert_eq!(config.accounts[1].name, "corp");
    assert_eq!(config.accounts[1].kind, AccountKind::M365);
    assert_eq!(config.accounts[1].tenant.as_deref(), Some("common"));
    assert_eq!(config.accounts[1].color.as_deref(), Some("magenta"));
    assert_eq!(config.accounts[1].client_secret, None);
}

#[test]
fn append_to_empty_text_creates_valid_config() {
    let out = append_account("", Path::new("c.toml"), &account("corp", "me@corp.com")).unwrap();
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.accounts.len(), 1);
}

#[test]
fn append_imapsmtp_account_writes_all_connection_fields_and_omits_client_id() {
    use crate::config::model::TlsMode;
    let acc = AccountConfig {
        name: "personal".into(),
        kind: AccountKind::ImapSmtp,
        address: "self@example.com".into(),
        imap_host: Some("imap.example.com".into()),
        imap_port: Some(993),
        imap_tls: Some(TlsMode::Required),
        imap_user: Some("self".into()),
        smtp_host: Some("smtp.example.com".into()),
        smtp_port: Some(465),
        smtp_tls: Some(TlsMode::Required),
        smtp_user: Some("self@example.com".into()),
        sent_folder: Some("Sent".into()),
        ..AccountConfig::default()
    };
    let out = append_account("", Path::new("c.toml"), &acc).unwrap();
    assert!(
        !out.contains("client_id"),
        "client_id should be omitted: {out}"
    );
    assert!(out.contains("imap_host = \"imap.example.com\""), "{out}");
    assert!(out.contains("imap_port = 993"), "{out}");
    assert!(out.contains("imap_tls = \"required\""), "{out}");
    assert!(out.contains("imap_user = \"self\""), "{out}");
    assert!(out.contains("smtp_host = \"smtp.example.com\""), "{out}");
    assert!(out.contains("smtp_port = 465"), "{out}");
    assert!(out.contains("smtp_tls = \"required\""), "{out}");
    assert!(out.contains("smtp_user = \"self@example.com\""), "{out}");
    assert!(out.contains("sent_folder = \"Sent\""), "{out}");

    let parsed = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(parsed.accounts.len(), 1);
    let p = &parsed.accounts[0];
    assert_eq!(p.name, "personal");
    assert_eq!(p.kind, AccountKind::ImapSmtp);
    assert_eq!(p.imap_port, Some(993));
    assert_eq!(p.smtp_tls, Some(TlsMode::Required));
}

#[test]
fn update_imapsmtp_account_modifies_fields_in_place_and_keeps_comments() {
    use crate::config::model::TlsMode;
    let initial = r#"# top comment
[[accounts]]
# my imap account
name = "personal"
kind = "imapsmtp"
address = "user@example.org"
imap_host = "old.example.org"
smtp_host = "old.example.org"
"#;
    let updated_acc = AccountConfig {
        name: "personal".into(),
        kind: AccountKind::ImapSmtp,
        address: "user@example.org".into(),
        imap_host: Some("imap.example.org".into()),
        imap_port: Some(993),
        imap_tls: Some(TlsMode::Required),
        smtp_host: Some("smtp.example.org".into()),
        smtp_port: Some(587),
        smtp_tls: Some(TlsMode::Opportunistic),
        sent_folder: Some("Sent Messages".into()),
        ..AccountConfig::default()
    };
    let out = update_account(initial, Path::new("c.toml"), &updated_acc).unwrap();
    assert!(out.contains("# top comment"), "{out}");
    assert!(out.contains("imap_host = \"imap.example.org\""), "{out}");
    assert!(out.contains("sent_folder = \"Sent Messages\""), "{out}");

    let parsed = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(
        parsed.accounts[0].imap_host.as_deref(),
        Some("imap.example.org")
    );
    assert_eq!(
        parsed.accounts[0].sent_folder.as_deref(),
        Some("Sent Messages")
    );
}

#[test]
fn append_rejects_unparseable_text() {
    let err =
        append_account("[[accounts", Path::new("c.toml"), &account("a", "a@b.c")).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
}

#[test]
fn set_secrets_keeps_comments_and_writes_the_onepassword_table() {
    let op = OnePasswordConfig {
        vault: "Rail".into(),
        account: Some("my.1password.com".into()),
        op_path: None,
    };
    let out = set_secrets(
        EXISTING,
        Path::new("c.toml"),
        SecretsBackend::OnePassword,
        Some(&op),
    )
    .unwrap();
    assert!(out.contains("# the good one"), "{out}");
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.general.secrets, SecretsBackend::OnePassword);
    assert_eq!(config.general.editor.as_deref(), Some("nvim"));
    assert_eq!(config.onepassword, Some(op));
    assert_eq!(config.accounts.len(), 1);
    let back = set_secrets(&out, Path::new("c.toml"), SecretsBackend::File, None).unwrap();
    let config = parse(&back, Path::new("c.toml")).unwrap();
    assert_eq!(config.general.secrets, SecretsBackend::File);
    assert!(
        config.onepassword.is_some(),
        "the table is kept for switching back"
    );
}

#[test]
fn set_secrets_updates_a_commented_onepassword_table_in_place() {
    let text = "[onepassword]\n# which vault\nvault = \"Old\" # trailing\nop_path = \"/usr/bin/op\"\n# keep me\n";
    let op = OnePasswordConfig {
        vault: "New".into(),
        account: Some("a.1password.com".into()),
        op_path: Some("/usr/bin/op".into()),
    };
    let out = set_secrets(
        text,
        Path::new("c.toml"),
        SecretsBackend::OnePassword,
        Some(&op),
    )
    .unwrap();
    for kept in ["# which vault", "# trailing", "# keep me", "/usr/bin/op"] {
        assert!(out.contains(kept), "{kept} lost:\n{out}");
    }
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.onepassword, Some(op));
}

#[test]
fn set_secrets_creates_the_general_table_and_refuses_1password_without_a_vault() {
    let out = set_secrets("", Path::new("c.toml"), SecretsBackend::File, None).unwrap();
    assert!(out.contains("secrets = \"file\""), "{out}");
    let err = set_secrets("", Path::new("c.toml"), SecretsBackend::OnePassword, None);
    assert!(matches!(err, Err(ConfigError::Invalid { .. })), "{err:?}");
}

#[test]
fn append_writes_sync_overrides_only_when_set() {
    let plain = append_account("", Path::new("c.toml"), &account("a", "a@x.com")).unwrap();
    assert!(!plain.contains("history_days"), "{plain}");
    let tuned = AccountConfig {
        history_days: Some(2),
        body_cache_days: Some(1),
        ..account("b", "b@x.com")
    };
    let out = append_account("", Path::new("c.toml"), &tuned).unwrap();
    assert!(out.contains("history_days = 2"), "{out}");
    assert!(out.contains("body_cache_days = 1"), "{out}");
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.accounts[0].history_days, Some(2));
}

#[test]
fn append_writes_from_name_only_when_set() {
    let unset = append_account("", Path::new("c.toml"), &account("a", "a@x.com")).unwrap();
    assert!(!unset.contains("from_name"), "{unset}");
    let named = AccountConfig {
        from_name: Some("Dana Reyes".into()),
        ..account("b", "b@x.com")
    };
    let out = append_account("", Path::new("c.toml"), &named).unwrap();
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.accounts[0].from_name.as_deref(), Some("Dana Reyes"));
}

#[test]
fn append_writes_ai_send_only_when_set() {
    let unset = append_account("", Path::new("c.toml"), &account("a", "a@x.com")).unwrap();
    assert!(!unset.contains("ai_send"), "{unset}");
    let drafts = AccountConfig {
        ai_send: Some(crate::config::AiSend::Drafts),
        ..account("b", "b@x.com")
    };
    let out = append_account("", Path::new("c.toml"), &drafts).unwrap();
    assert!(out.contains("ai_send = \"drafts\""), "{out}");
    let config = parse(&out, Path::new("c.toml")).unwrap();
    assert_eq!(config.accounts[0].ai_send(), crate::config::AiSend::Drafts);
}

#[test]
fn remove_account_keeps_comments_and_the_other_accounts() {
    let two = append_account(
        EXISTING,
        Path::new("c.toml"),
        &account("corp", "me@corp.com"),
    )
    .unwrap();
    let out = remove_account(&two, Path::new("c.toml"), "work").unwrap();
    assert!(out.contains("# my rail config"), "{out}");
    assert!(out.contains("# the good one"), "{out}");
    let config = parse(&out, Path::new("c.toml")).unwrap();
    let names: Vec<_> = config.accounts.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["corp"]);
    let last = remove_account(&out, Path::new("c.toml"), "corp").unwrap();
    assert!(
        parse(&last, Path::new("c.toml"))
            .unwrap()
            .accounts
            .is_empty()
    );
    let err = remove_account(&last, Path::new("c.toml"), "corp").unwrap_err();
    assert!(err.to_string().contains("no [[accounts]] entry"), "{err}");
}

#[test]
fn default_name_is_local_part_made_unique() {
    let mut config = Config::default();
    assert_eq!(
        default_account_name("Dana.Reyes+x@corp.com", &config),
        "dana_reyes_x"
    );
    config
        .accounts
        .push(account("dana_reyes_x", "other@corp.com"));
    assert_eq!(
        default_account_name("dana.reyes+x@corp.com", &config),
        "dana_reyes_x-2"
    );
}

#[test]
fn set_general_changes_one_key_and_keeps_everything_else() {
    let text = "# my rail config\n[general]\neditor = \"nvim\" # the good one\nicons = \"ascii\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"g\"\n";
    let path = Path::new("c.toml");
    let out = set_general(text, path, "theme", Some("Dracula".into())).unwrap();
    let expected = text.replace(
        "icons = \"ascii\"\n",
        "icons = \"ascii\"\ntheme = \"Dracula\"\n",
    );
    assert_eq!(out, expected);
    let config = parse(&out, path).unwrap();
    assert_eq!(config.general.theme.as_deref(), Some("Dracula"));

    let again = set_general(&out, path, "theme", Some("Nord".into())).unwrap();
    assert_eq!(again, out.replace("\"Dracula\"", "\"Nord\""));

    let slot = set_general(&again, path, "accent", Some(5i64.into())).unwrap();
    assert!(slot.contains("accent = 5\n"), "{slot}");

    let removed = set_general(&slot, path, "theme", None).unwrap();
    assert!(!removed.contains("theme"), "{removed}");
    assert!(removed.contains("# the good one"), "{removed}");
    assert_eq!(
        set_general(&removed, path, "theme", None).unwrap(),
        removed,
        "removing an absent key changes nothing"
    );
}

#[test]
fn set_general_creates_the_table_and_rejects_a_value_the_config_refuses() {
    let out = set_general("", Path::new("c.toml"), "theme", Some("Nord".into())).unwrap();
    assert_eq!(out, "[general]\ntheme = \"Nord\"\n");
    let err = set_general("", Path::new("c.toml"), "accent", Some(99i64.into()));
    assert!(matches!(err, Err(ConfigError::Parse { .. })), "{err:?}");
}

#[test]
fn the_text_and_number_wrappers_round_trip() {
    let path = Path::new("c.toml");
    let out = set_general_text("", path, "theme", Some("Nord")).unwrap();
    assert_eq!(out, "[general]\ntheme = \"Nord\"\n");
    assert_eq!(
        set_general_text(&out, path, "theme", None).unwrap(),
        "[general]\n"
    );
    let out = set_general_number("", path, "accent", Some(7)).unwrap();
    assert_eq!(out, "[general]\naccent = 7\n");
    assert_eq!(
        set_general_number(&out, path, "accent", None).unwrap(),
        "[general]\n"
    );
}

#[test]
fn set_general_bool_writes_false_keeps_the_rest_and_none_removes_it() {
    let text = "# my rail config\n[general]\neditor = \"nvim\" # the good one\nicons = \"ascii\"\n";
    let path = Path::new("c.toml");
    let out = set_general_bool(text, path, "hints", Some(false)).unwrap();
    assert_eq!(out, format!("{text}hints = false\n"));
    let config = parse(&out, path).unwrap();
    assert!(!config.general.hints);
    assert_eq!(config.general.editor.as_deref(), Some("nvim"));
    let removed = set_general_bool(&out, path, "hints", None).unwrap();
    assert_eq!(removed, text, "the key goes and every comment stays");
    assert!(parse(&removed, path).unwrap().general.hints);
}

#[test]
fn set_general_text_saves_a_spaced_theme_and_refuses_control_and_format_characters() {
    let path = Path::new("c.toml");
    let out = set_general_text("", path, "theme", Some("Catppuccin Mocha")).unwrap();
    assert_eq!(
        parse(&out, path).unwrap().general.theme.as_deref(),
        Some("Catppuccin Mocha")
    );
    for bad in ["a\tb", "a\u{202E}b"] {
        let err = set_general_text("", path, "theme", Some(bad)).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("theme"), "{message}");
        assert!(
            !message.contains(bad.chars().nth(1).unwrap()),
            "{message:?}"
        );
    }
}

/// AC-24 (FR-20): the user's comments and every other table survive a save, a rename and a
/// removal, and a rename edits its table **in place** so the sidebar row keeps its
/// position (D-5).
#[test]
fn saved_search_edits_keep_comments() {
    let path = Path::new("c.toml");
    let text = "# my rail config\n[general]\neditor = \"nvim\" # the good one\n\n# the one that matters\n[[saved_searches]]\nname = \"needs-reply\"\nquery = \"from:dana is:unread\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"g\"\n";
    assert_eq!(parse(text, path).unwrap().saved_searches.len(), 1);

    let added = put_saved_search(text, path, "big", Some("has:attachment"), None).unwrap();
    assert!(added.contains("# my rail config"), "{added}");
    assert!(
        added.contains("editor = \"nvim\" # the good one"),
        "{added}"
    );
    assert!(added.contains("# the one that matters"), "{added}");
    let config = parse(&added, path).unwrap();
    let names: Vec<_> = config
        .saved_searches
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["needs-reply", "big"], "a new search is appended");
    assert_eq!(config.saved_searches[1].query, "has:attachment");
    assert_eq!(config.accounts.len(), 1, "other tables are untouched");

    // A rename keeps the entry's position, its query and the comment above it.
    let renamed = put_saved_search(
        &added,
        path,
        "later",
        Some("from:dana is:unread"),
        Some("needs-reply"),
    )
    .unwrap();
    assert!(renamed.contains("# the one that matters"), "{renamed}");
    let config = parse(&renamed, path).unwrap();
    let names: Vec<_> = config
        .saved_searches
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["later", "big"], "renamed in place, not re-appended");
    assert_eq!(config.saved_searches[0].query, "from:dana is:unread");

    let gone = remove_saved_search(&renamed, path, "later").unwrap();
    assert!(gone.contains("# my rail config"), "{gone}");
    assert!(gone.contains("editor = \"nvim\" # the good one"), "{gone}");
    let config = parse(&gone, path).unwrap();
    let names: Vec<_> = config
        .saved_searches
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["big"]);

    let last = remove_saved_search(&gone, path, "big").unwrap();
    let config = parse(&last, path).unwrap();
    // Counted, not shown: `SavedSearch` has no `Debug` (§8 Privacy), so `assert_eq!`
    // against an empty slice would need one.
    assert!(
        config.saved_searches.is_empty(),
        "{} left",
        config.saved_searches.len()
    );
    assert!(!last.contains("[[saved_searches]]"), "{last}");
    assert_eq!(config.accounts.len(), 1);

    let err = remove_saved_search(&last, path, "big").unwrap_err();
    assert!(
        err.to_string().contains("no [[saved_searches]] entry"),
        "{err}"
    );
    let err = put_saved_search(&last, path, "x", Some("is:unread"), Some("gone")).unwrap_err();
    assert!(
        err.to_string().contains("no [[saved_searches]] entry"),
        "{err}"
    );
}

/// FR-20 and docs/configuration.md's "your comments, formatting and other tables survive
/// each write", on the two lines a rename touches. Assigning
/// `table["name"]`/`table["query"]` replaces the item together with its decor, so the
/// trailing comment on each line goes and a literal query is re-quoted; `set_string` keeps
/// the decor, and an unchanged query is not written at all, which is the only thing that
/// keeps its `'…'` form. A rename to the same name writes nothing, so the file comes back
/// byte for byte.
#[test]
fn a_rename_keeps_the_comments_and_quoting_of_its_lines() {
    let path = Path::new("c.toml");
    let text = "[[saved_searches]]\nname = \"needs-reply\" # triage queue\nquery = 'from:dana is:unread' # weekly review\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"g\"\n";
    let config = parse(text, path).unwrap();
    assert_eq!(config.saved_searches[0].query, "from:dana is:unread");

    let renamed = put_saved_search(
        text,
        path,
        "later",
        Some("from:dana is:unread"),
        Some("needs-reply"),
    )
    .unwrap();
    assert!(
        renamed.contains("name = \"later\" # triage queue"),
        "the name's own comment stays: {renamed}"
    );
    assert!(
        renamed.contains("query = 'from:dana is:unread' # weekly review"),
        "an unchanged query keeps its comment and its single quotes: {renamed}"
    );
    assert_eq!(parse(&renamed, path).unwrap().accounts.len(), 1);

    // A query that did change is written, and keeps its line's comment.
    let requeried =
        put_saved_search(&renamed, path, "later", Some("from:dana"), Some("later")).unwrap();
    assert!(
        requeried.contains("query = \"from:dana\" # weekly review"),
        "{requeried}"
    );

    let same = put_saved_search(
        text,
        path,
        "needs-reply",
        Some("from:dana is:unread"),
        Some("needs-reply"),
    )
    .unwrap();
    assert_eq!(same, text, "a rename to the same name writes nothing");
}

/// FR-20's other half, for the **name** line. `set_string` keeps a line's trailing comment
/// but cannot keep a literal string's `'…'` form, so writing a name that did not change
/// re-quotes `'later'` to `"later"` and edits a line the user asked nothing of. The
/// byte-for-byte check above cannot see this: its name is a basic string, where writing
/// the same value back is invisible. A case-only rename is a real change and is still
/// written.
#[test]
fn a_rename_that_does_not_change_the_name_leaves_it_as_written() {
    let path = Path::new("c.toml");
    let text = "[[saved_searches]]\nname = 'later' # literal name\nquery = 'from:dana' # literal\n";
    let same = put_saved_search(text, path, "later", Some("from:dana"), Some("later")).unwrap();
    assert_eq!(same, text, "a no-op rename writes neither line");

    let cased = put_saved_search(text, path, "LATER", Some("from:dana"), Some("later")).unwrap();
    assert!(
        cased.contains("name = \"LATER\" # literal name"),
        "a case-only rename is written, and keeps its line's comment: {cased}"
    );
    assert_eq!(parse(&cased, path).unwrap().saved_searches[0].name, "LATER");
}

/// #109: a rename with no query leaves the query line exactly as the file has it, even
/// when it was edited by hand after the caller last read it.
#[test]
fn a_rename_without_a_query_keeps_the_files_query() {
    let path = Path::new("c.toml");
    let text =
        "[[saved_searches]]\nname = \"later\"\n# by hand\nquery = 'from:dana is:unread' # mine\n";
    let renamed = put_saved_search(text, path, "soon", None, Some("later")).unwrap();
    assert_eq!(renamed, text.replace("\"later\"", "\"soon\""));
    assert!(
        put_saved_search(text, path, "new", None, None).is_err(),
        "a new one needs a query"
    );
}
