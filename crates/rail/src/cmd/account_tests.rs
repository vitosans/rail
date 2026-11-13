use super::*;
use rail_core::config::AiSend;
use rail_core::ops::Secret;

fn parse_add(args: &[&str]) -> AddArgs {
    use crate::cli::{AccountCommand, Cli, Command};
    use clap::Parser;
    match Cli::try_parse_from(args).unwrap().command {
        Some(Command::Account {
            command: AccountCommand::Add(add),
        }) => *add,
        other => panic!("{other:?}"),
    }
}

fn from_args(args: &[&str]) -> color_eyre::Result<Option<NewAccount>> {
    let mut all = vec!["rail", "account", "add"];
    all.extend_from_slice(args);
    new_account_from_args(parse_add(&all))
}

fn oauth_from_args(args: &[&str]) -> AccountAddRequest {
    match from_args(args).unwrap().unwrap() {
        NewAccount::OAuth(req) => req,
        NewAccount::ImapSmtp(_) => panic!("expected OAuth request"),
    }
}

fn message(args: &[&str]) -> String {
    from_args(args)
        .expect_err("partial set refused")
        .to_string()
}

#[test]
fn no_provider_and_no_flags_opens_the_wizard() {
    assert!(from_args(&[]).unwrap().is_none());
}

#[test]
fn a_provider_alone_opens_the_wizard() {
    assert!(from_args(&["gmail"]).unwrap().is_none());
    assert!(from_args(&["imapsmtp"]).unwrap().is_none());
}

#[test]
fn address_and_client_id_with_a_provider_build_the_account() {
    let flags = [
        "m365",
        "--address",
        "a@b.c",
        "--client-id",
        "x",
        "--tenant",
        "t",
    ];
    let new = oauth_from_args(&flags);
    assert_eq!(new.kind, AccountKind::M365);
    assert_eq!(new.address, "a@b.c");
    assert_eq!(new.client_id.as_deref(), Some("x"));
    assert_eq!(new.tenant.as_deref(), Some("t"));
    assert_eq!(new.name, None);
    assert!(new.open_browser);
}

#[test]
fn a_client_secret_flag_may_be_a_1password_reference() {
    let base = [
        "gmail",
        "--address",
        "a@b.c",
        "--client-id",
        "x",
        "--client-secret",
    ];
    let mut reference = base.to_vec();
    reference.push("op://Rail/google/client_secret");
    assert_eq!(
        oauth_from_args(&reference).client_secret,
        Some(ClientSecretInput::Reference(
            "op://Rail/google/client_secret".into()
        ))
    );
    let mut value = base.to_vec();
    value.push("GOCSPX-x");
    assert_eq!(
        oauth_from_args(&value).client_secret,
        Some(ClientSecretInput::Value(Secret::new("GOCSPX-x")))
    );
}

/// Final review F4: the flag reaches the request; without it the request stays false. On
/// its own (no --address) it names the missing flag like every other.
#[test]
fn store_secret_in_1password_maps_to_the_request() {
    let base = [
        "gmail",
        "--address",
        "a@b.c",
        "--client-id",
        "x",
        "--client-secret",
        "GOCSPX-x",
    ];
    assert!(!oauth_from_args(&base).store_secret_in_1password);
    let mut flagged = base.to_vec();
    flagged.push("--store-secret-in-1password");
    assert!(oauth_from_args(&flagged).store_secret_in_1password);
    let msg = message(&["gmail", "--store-secret-in-1password"]);
    assert!(
        msg.contains("--store-secret-in-1password needs --address"),
        "{msg}"
    );
}

#[test]
fn address_without_a_provider_names_the_provider() {
    let msg = message(&["--address", "a@b.c", "--client-id", "x"]);
    assert!(msg.contains("provider"), "{msg}");
}

#[test]
fn address_without_a_client_id_names_the_flag() {
    let msg = message(&["gmail", "--address", "a@b.c"]);
    assert!(msg.contains("--client-id"), "{msg}");
}

#[test]
fn any_other_flag_without_an_address_names_the_address_flag() {
    for flag in [
        ["--client-id", "x"],
        ["--client-secret", "x"],
        ["--tenant", "x"],
        ["--name", "x"],
        ["--color", "x"],
    ] {
        let msg = message(&["gmail", flag[0], flag[1]]);
        assert!(msg.contains("--address"), "{flag:?}: {msg}");
    }
}

#[test]
fn a_report_carries_the_hint() {
    let error = OpError::invalid_input("bad").with_hint("do this");
    assert_eq!(report(error).to_string(), "bad\nhint: do this");
}
/// Parked item 1: Rail's own message and hint are escaped too, as defence in depth.
#[test]
fn control_characters_in_message_and_hint_are_escaped() {
    let error = OpError::invalid_input("bad \u{1b}[2J message").with_hint("run\n\u{7}this");
    let text = error_text(&error);
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["bad \\u{1b}[2J message", "hint: run\\n\\u{7}this"]);
}

#[test]
fn outside_detail_is_quoted_escaped_and_on_its_own_line() {
    let error = OpError::invalid_input("the provider refused")
        .with_detail("bad\nline \u{1b}[2J \"quoted\"")
        .with_hint("try again");
    let text = error_text(&error);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0], "the provider refused");
    assert_eq!(
        lines[1],
        r#"from outside Rail (the provider or the system): "bad\nline \u{1b}[2J \"quoted\"""#
    );
    assert_eq!(lines[2], "hint: try again");
    assert!(!text.contains('\u{1b}'), "{text:?}");
}

fn summary(name: &str, address: &str, tokens: TokenState) -> AccountSummary {
    AccountSummary {
        name: name.into(),
        kind: AccountKind::Gmail,
        address: address.into(),
        color: None,
        tokens,
        ai_send: AiSend::Confirm,
    }
}

#[test]
fn long_names_and_addresses_keep_the_columns_aligned() {
    let text = table(&[
        summary(
            "a-very-long-account-name",
            "someone.with.a.long.address@example.com",
            TokenState::Present,
        ),
        summary("w", "w@x.io", TokenState::Present),
    ]);
    let lines: Vec<&str> = text.lines().collect();
    let column = |line: &str, word: &str| line.find(word).unwrap();
    assert_eq!(column(lines[0], "KIND"), column(lines[1], "gmail"));
    assert_eq!(column(lines[1], "gmail"), column(lines[2], "gmail"));
    assert_eq!(column(lines[0], "TOKENS"), column(lines[2], "yes"));
    assert!(lines[2].starts_with("w "), "{text}");
}

#[test]
fn a_token_store_error_in_the_table_is_quoted() {
    let message = "locked\n\u{1b}[31m".to_string().into();
    let text = table(&[summary("w", "w@x.io", TokenState::Error { message })]);
    assert_eq!(text.lines().count(), 2, "{text:?}");
    assert!(
        text.contains(
            r#"error from outside Rail (the provider or the system): "locked\n\u{1b}[31m""#
        ),
        "{text:?}"
    );
    assert!(!text.contains('\u{1b}'), "{text:?}");
}

#[test]
fn an_unanswered_cancel_says_the_login_may_still_be_pending() {
    let text = cancel_unanswered(CANCEL_TIMEOUT).to_string();
    assert!(text.starts_with("the daemon did not answer the cancel within 5 seconds"));
    assert!(text.contains("the login may still be pending"), "{text}");
    assert!(text.contains("\nhint: run `rail daemon status`"), "{text}");
}

#[test]
fn imapsmtp_flags_build_imapsmtp_account() {
    let flags = [
        "imapsmtp",
        "--address",
        "user@example.com",
        "--imap-host",
        "imap.example.com",
        "--smtp-host",
        "smtp.example.com",
        "--imap-port",
        "993",
        "--smtp-port",
        "465",
    ];
    let new = from_args(&flags).unwrap().unwrap();
    match new {
        NewAccount::ImapSmtp(args) => {
            assert_eq!(args.address, "user@example.com");
            assert_eq!(args.imap_host, "imap.example.com");
            assert_eq!(args.smtp_host, "smtp.example.com");
            assert_eq!(args.imap_port, Some(993));
            assert_eq!(args.smtp_port, Some(465));
        }
        NewAccount::OAuth(_) => panic!("expected ImapSmtp"),
    }
}

#[test]
fn imapsmtp_missing_host_flags_are_reported() {
    let no_imap = message(&[
        "imapsmtp",
        "--address",
        "user@example.com",
        "--smtp-host",
        "smtp.example.com",
    ]);
    assert!(
        no_imap.contains("imapsmtp accounts need --imap-host"),
        "{no_imap}"
    );

    let no_smtp = message(&[
        "imapsmtp",
        "--address",
        "user@example.com",
        "--imap-host",
        "imap.example.com",
    ]);
    assert!(
        no_smtp.contains("imapsmtp accounts need --smtp-host"),
        "{no_smtp}"
    );
}
