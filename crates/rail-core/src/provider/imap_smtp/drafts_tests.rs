use super::client::{ImapStream, classify_imap_error};
use super::drafts::*;
use crate::provider::{Mailbox, Outgoing, OutgoingMail, ProviderError};
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
            subject: "Draft Subject".to_string(),
            message_id: msg_id.to_string(),
            in_reply_to: None,
            references: vec![],
            body: "Draft body text".to_string(),
            attachments: vec![],
        },
        thread: None,
        remote: None,
        kind: crate::store::drafts::DraftKind::New,
        source: None,
    }
}

#[tokio::test]
async fn test_save_draft_append_and_find() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];

        // 1. APPEND "Drafts" (\Draft \Seen) {size}
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("APPEND"));
        let tag = cmd.split_whitespace().next().unwrap().to_string();
        // Server asks for literal bytes
        server_io
            .write_all(b"+ Ready for literal data\r\n")
            .await
            .unwrap();

        // Read literal bytes
        let _ = server_io.read(&mut buf).await.unwrap();
        let resp = format!("{tag} OK [APPENDUID 999 50] APPEND completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. EXAMINE "Drafts"
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("EXAMINE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!(
            "* 1 EXISTS\r\n* OK [UIDVALIDITY 999] UIDs valid\r\n{tag} OK [READ-ONLY] EXAMINE completed\r\n"
        );
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 3. UID SEARCH HEADER Message-ID "draft001@example.com"
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID SEARCH"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* SEARCH 50\r\n{tag} OK SEARCH completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let mail = sample_outgoing_mail("<draft001@example.com>");
    let saved = save_draft(&mut session, "Drafts", &mail).await.unwrap();

    assert_eq!(saved.ids.draft_id, "Drafts:999:50");
    assert_eq!(saved.ids.message_id, "Drafts:999:50");
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_delete_draft_success() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // 1. SELECT "Drafts"
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("SELECT"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("{tag} OK SELECT completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. UID STORE 50 +FLAGS (\Deleted)
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID STORE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* 1 FETCH (UID 50 FLAGS (\\Deleted))\r\n{tag} OK STORE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 3. EXPUNGE
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("EXPUNGE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* 1 EXPUNGE\r\n{tag} OK EXPUNGE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let result = delete_draft(&mut session, "Drafts:999:50").await;
    assert!(result.is_ok());
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_delete_draft_not_found() {
    let (mut session, mut server_io) = setup_mock_session().await;

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // 1. SELECT "Drafts"
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = String::from_utf8_lossy(&buf[..n])
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        server_io
            .write_all(format!("{tag} OK SELECT\r\n").as_bytes())
            .await
            .unwrap();

        // 2. UID STORE 999 +FLAGS (\Deleted) -> returns no fetch items
        let n = server_io.read(&mut buf).await.unwrap();
        let tag = String::from_utf8_lossy(&buf[..n])
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        server_io
            .write_all(format!("{tag} OK STORE\r\n").as_bytes())
            .await
            .unwrap();
    });

    let result = delete_draft(&mut session, "Drafts:999:999").await;
    assert!(matches!(result, Err(ProviderError::NotFound)));
    server_task.await.unwrap();
}
