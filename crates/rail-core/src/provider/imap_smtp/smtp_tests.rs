use lettre::address::Address;

use super::smtp::{build_smtp_transport, collect_recipient_addresses, default_smtp_port};
use crate::config::model::TlsMode;
use crate::ops::Secret;
use crate::provider::types::{Mailbox, Outgoing, OutgoingMail};
use crate::store::drafts::DraftKind;

#[test]
fn test_default_smtp_ports() {
    assert_eq!(default_smtp_port(TlsMode::Required), 465);
    assert_eq!(default_smtp_port(TlsMode::Opportunistic), 587);
    assert_eq!(default_smtp_port(TlsMode::Off), 25);
}

#[tokio::test]
async fn test_build_smtp_transport_configurations() {
    let pass = Secret::new("secret123".to_string());

    // Required TLS (port 465 -> wrapper)
    let t1 = build_smtp_transport(
        "smtp.example.com",
        Some(465),
        TlsMode::Required,
        Some("user@example.com"),
        Some(&pass),
    );
    assert!(t1.is_ok());

    // Required TLS (port 587 -> STARTTLS)
    let t2 = build_smtp_transport(
        "smtp.example.com",
        Some(587),
        TlsMode::Required,
        Some("user@example.com"),
        Some(&pass),
    );
    assert!(t2.is_ok());

    // Opportunistic TLS
    let t3 = build_smtp_transport("smtp.example.com", None, TlsMode::Opportunistic, None, None);
    assert!(t3.is_ok());

    // Plain TLS off
    let t4 = build_smtp_transport("127.0.0.1", Some(2525), TlsMode::Off, None, None);
    assert!(t4.is_ok());
}

#[test]
fn test_collect_recipient_addresses() {
    let mail = OutgoingMail {
        outgoing: Outgoing {
            from_name: Some("Sender".into()),
            from: "sender@example.com".into(),
            to: vec![Mailbox {
                name: Some("To".into()),
                address: "to@example.com".into(),
            }],
            cc: vec![Mailbox {
                name: None,
                address: "cc@example.com".into(),
            }],
            bcc: vec![Mailbox {
                name: None,
                address: "bcc@example.com".into(),
            }],
            subject: "Test".into(),
            message_id: "<msg123@example.com>".into(),
            in_reply_to: None,
            references: vec![],
            body: "Hello".into(),
            attachments: vec![],
        },
        kind: DraftKind::New,
        source: None,
        remote: None,
        thread: None,
    };

    let addrs = collect_recipient_addresses(&mail).unwrap();
    assert_eq!(addrs.len(), 3);
    assert_eq!(addrs[0], "to@example.com".parse::<Address>().unwrap());
    assert_eq!(addrs[1], "cc@example.com".parse::<Address>().unwrap());
    assert_eq!(addrs[2], "bcc@example.com".parse::<Address>().unwrap());

    // Empty recipients
    let empty_mail = OutgoingMail {
        outgoing: Outgoing {
            from_name: None,
            from: "sender@example.com".into(),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "".into(),
            message_id: "<id>".into(),
            in_reply_to: None,
            references: vec![],
            body: "".into(),
            attachments: vec![],
        },
        kind: DraftKind::New,
        source: None,
        remote: None,
        thread: None,
    };
    let empty_addrs = collect_recipient_addresses(&empty_mail).unwrap();
    assert!(empty_addrs.is_empty());

    // Invalid recipient
    let bad_mail = OutgoingMail {
        outgoing: Outgoing {
            from_name: None,
            from: "sender@example.com".into(),
            to: vec![Mailbox {
                name: None,
                address: "not an email".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "".into(),
            message_id: "<id>".into(),
            in_reply_to: None,
            references: vec![],
            body: "".into(),
            attachments: vec![],
        },
        kind: DraftKind::New,
        source: None,
        remote: None,
        thread: None,
    };
    assert!(collect_recipient_addresses(&bad_mail).is_err());
}

#[tokio::test]
async fn test_classify_smtp_connection_error() {
    use super::smtp::validate_smtp_connection;
    use crate::error::ErrorClass;

    // Connect to unroutable port on localhost to produce real SmtpError
    let transport = build_smtp_transport("127.0.0.1", Some(1), TlsMode::Off, None, None).unwrap();
    let err = validate_smtp_connection(&transport).await.unwrap_err();
    assert_eq!(err.class(), ErrorClass::Transient);
}
