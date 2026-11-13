use super::client::{ImapStream, classify_imap_error};
use super::send::*;
use super::smtp::build_smtp_transport;
use crate::config::model::TlsMode;
use crate::provider::{Mailbox, Outgoing, OutgoingMail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn setup_mock_session() -> (async_imap::Session<ImapStream>, tokio::io::DuplexStream) {
    let (client_io, mut server_io) = tokio::io::duplex(4096);
    let mut client = async_imap::Client::new(ImapStream::Mock(client_io));

    server_io
        .write_all(b"* OK Mock IMAP Server Ready\r\n")
        .await
        .unwrap();
    client.read_response().await.unwrap();

    let login_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        let tag = cmd.split_whitespace().next().unwrap_or("a0000");
        let resp = format!("{tag} OK LOGIN completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
        server_io
    });

    let (session, _) = client
        .login_with_capabilities("user", "pass")
        .await
        .map_err(|(e, _)| classify_imap_error(&e))
        .unwrap();

    let server_io = login_task.await.unwrap();
    (session, server_io)
}

fn sample_outgoing_mail(msg_id: &str) -> OutgoingMail {
    OutgoingMail {
        outgoing: Outgoing {
            from_name: Some("Alice".to_string()),
            from: "alice@example.com".to_string(),
            to: vec![Mailbox {
                name: Some("Bob".to_string()),
                address: "bob@example.com".to_string(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Send Subject".to_string(),
            message_id: msg_id.to_string(),
            in_reply_to: None,
            references: vec![],
            body: "Send body text".to_string(),
            attachments: vec![],
        },
        thread: None,
        remote: None,
        kind: crate::store::drafts::DraftKind::New,
        source: None,
    }
}

#[tokio::test]
async fn test_check_sent_already_found() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // 1. EXAMINE "Sent"
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("EXAMINE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!(
            "* 1 EXISTS\r\n* OK [UIDVALIDITY 777] UIDs valid\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n"
        );
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. UID SEARCH HEADER Message-ID "sent001@example.com"
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID SEARCH"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* SEARCH 10\r\n{tag} OK SEARCH completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let already = check_sent_already(&mut session, "Sent", "<sent001@example.com>")
        .await
        .unwrap();
    assert!(already);
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_check_sent_already_not_found() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // 1. EXAMINE "Sent"
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = cmd_tag(&buf[..n]);
        let resp = format!(
            "* 0 EXISTS\r\n* OK [UIDVALIDITY 777] UIDs valid\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n"
        );
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. UID SEARCH
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = cmd_tag(&buf[..n]);
        let resp = format!("* SEARCH\r\n{tag} OK SEARCH completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let already = check_sent_already(&mut session, "Sent", "<not_found@example.com>")
        .await
        .unwrap();
    assert!(!already);
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_append_to_sent_success() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];

        // 1. APPEND "Sent" (\Seen)
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = cmd_tag(&buf[..n]);
        server_io.write_all(b"+ Ready\r\n").await.unwrap();

        let _ = server_io.read(&mut buf).await.unwrap();
        let resp = format!("{tag} OK APPEND completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. EXAMINE "Sent"
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = cmd_tag(&buf[..n]);
        let resp = format!(
            "* 1 EXISTS\r\n* OK [UIDVALIDITY 777] UIDs valid\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n"
        );
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 3. UID SEARCH
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = cmd_tag(&buf[..n]);
        let resp = format!("* SEARCH 15\r\n{tag} OK SEARCH completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let mail = sample_outgoing_mail("<sent_msg@example.com>");
    let sent_id = append_to_sent(&mut session, "Sent", &mail).await.unwrap();
    assert_eq!(sent_id, Some("Sent:777:15".to_string()));
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_send_two_phase_smtp_failure_does_not_append_sent() {
    // Unreachable SMTP port triggers immediate error
    let transport = build_smtp_transport("127.0.0.1", Some(1), TlsMode::Off, None, None).unwrap();
    let (mut session, _server_io) = setup_mock_session().await;

    let mail = sample_outgoing_mail("<fail_smtp@example.com>");
    let result = send_two_phase(&transport, Some(&mut session), "Sent", &mail).await;

    // Must return Err on SMTP failure
    assert!(result.is_err());
}

fn cmd_tag(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .next()
        .unwrap_or("A0000")
        .to_string()
}
