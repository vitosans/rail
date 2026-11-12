use super::identity::{
    ImapMessageId, UidValidityCheck, check_uidvalidity, clean_message_id, compute_thread_id,
};

#[test]
fn test_imap_message_id_formatting_and_parsing() {
    let msg_id = ImapMessageId::new("INBOX", 1698765432, 42);
    let remote = msg_id.to_remote_id();
    assert_eq!(remote, "INBOX:1698765432:42");

    let parsed = ImapMessageId::parse(&remote).unwrap();
    assert_eq!(parsed, msg_id);

    // Mailbox name with colons
    let complex = ImapMessageId::new("Special:Archive:2024", 9999, 105);
    let complex_remote = complex.to_remote_id();
    assert_eq!(complex_remote, "Special:Archive:2024:9999:105");
    let complex_parsed = ImapMessageId::parse(&complex_remote).unwrap();
    assert_eq!(complex_parsed.mailbox, "Special:Archive:2024");
    assert_eq!(complex_parsed.uidvalidity, 9999);
    assert_eq!(complex_parsed.uid, 105);

    // Invalid formats
    assert!(ImapMessageId::parse("").is_none());
    assert!(ImapMessageId::parse("INBOX:42").is_none());
    assert!(ImapMessageId::parse("INBOX:not_num:42").is_none());
    assert!(ImapMessageId::parse("INBOX:123:not_num").is_none());
    assert!(ImapMessageId::parse(":123:42").is_none());
}

#[test]
fn test_clean_message_id() {
    assert_eq!(
        clean_message_id("<foo@example.com>"),
        Some("foo@example.com".into())
    );
    assert_eq!(
        clean_message_id("  <foo@example.com>  "),
        Some("foo@example.com".into())
    );
    assert_eq!(
        clean_message_id("foo@example.com"),
        Some("foo@example.com".into())
    );
    assert_eq!(clean_message_id("<>"), None);
    assert_eq!(clean_message_id("   "), None);
}

#[test]
fn test_compute_thread_id_threading_chains() {
    let root_msg_id = "<root@example.com>";
    let reply1_msg_id = "<reply1@example.com>";
    let reply2_msg_id = "<reply2@example.com>";

    // 1. Root message
    let root_thread = compute_thread_id(root_msg_id, None, &[]);

    // 2. Reply 1 referencing root in in_reply_to and references
    let reply1_thread =
        compute_thread_id(reply1_msg_id, Some(root_msg_id), &[root_msg_id.to_string()]);

    // 3. Reply 2 referencing root as first item in references
    let reply2_thread = compute_thread_id(
        reply2_msg_id,
        Some(reply1_msg_id),
        &[root_msg_id.to_string(), reply1_msg_id.to_string()],
    );

    // All belong to the same thread ID
    assert_eq!(root_thread, reply1_thread);
    assert_eq!(root_thread, reply2_thread);

    // Reply with only In-Reply-To
    let reply_in_reply_to_only =
        compute_thread_id("<other_reply@example.com>", Some(root_msg_id), &[]);
    assert_eq!(root_thread, reply_in_reply_to_only);

    // Distinct root should have different thread ID
    let distinct_thread = compute_thread_id("<distinct@example.com>", None, &[]);
    assert_ne!(root_thread, distinct_thread);

    // Case-insensitivity in Message-IDs
    let root_upper_thread = compute_thread_id("<ROOT@EXAMPLE.COM>", None, &[]);
    assert_eq!(root_thread, root_upper_thread);
}

#[test]
fn test_check_uidvalidity_epochs() {
    assert_eq!(
        check_uidvalidity(None, 12345),
        UidValidityCheck::Initial(12345)
    );
    assert_eq!(
        check_uidvalidity(Some(12345), 12345),
        UidValidityCheck::Unchanged
    );
    assert_eq!(
        check_uidvalidity(Some(12345), 67890),
        UidValidityCheck::Reset {
            previous: 12345,
            current: 67890,
        }
    );
}
