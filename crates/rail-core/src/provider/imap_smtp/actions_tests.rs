use super::actions::*;
use super::client::{ImapCapabilities, ImapStream, classify_imap_error};
use super::folders::DiscoveredFolders;
use crate::error::ErrorClass;
use crate::provider::{Change, FolderKind, RemoteFolder, TargetMessage, ThreadTarget};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn setup_mock_session() -> (async_imap::Session<ImapStream>, tokio::io::DuplexStream) {
    let (client_io, mut server_io) = tokio::io::duplex(4096);
    let mut client = async_imap::Client::new(ImapStream::Mock(client_io));

    // Handle greeting
    server_io
        .write_all(b"* OK Mock IMAP Server Ready\r\n")
        .await
        .unwrap();
    client.read_response().await.unwrap();

    // Spawn task to answer login
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

#[tokio::test]
async fn test_labels_rejected() {
    let (mut session, _server_io) = setup_mock_session().await;
    let caps = ImapCapabilities::default();
    let folders = DiscoveredFolders::new(vec![]);
    let threads = vec![ThreadTarget {
        thread_id: "thread:1".to_string(),
        messages: vec![TargetMessage {
            id: "INBOX:1:1".to_string(),
            folder: Some(FolderKind::Inbox),
            unread: true,
            ..Default::default()
        }],
    }];
    let change = Change::Labels {
        add: vec!["label1".to_string()],
        remove: vec![],
    };

    let result = apply_change(&mut session, &caps, &folders, &threads, &change).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.class(), ErrorClass::Permanent);
}

#[tokio::test]
async fn test_read_flag_update() {
    let (mut session, mut server_io) = setup_mock_session().await;
    let caps = ImapCapabilities::default();
    let folders = DiscoveredFolders::new(vec![]);
    let threads = vec![ThreadTarget {
        thread_id: "thread:1".to_string(),
        messages: vec![TargetMessage {
            id: "INBOX:100:42".to_string(),
            folder: Some(FolderKind::Inbox),
            unread: true,
            ..Default::default()
        }],
    }];

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // 1. SELECT INBOX
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        eprintln!("DEBUG SELECT cmd: {:?}", cmd);
        assert!(cmd.to_uppercase().contains("SELECT"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!(
            "* 1 EXISTS\r\n* OK [UIDVALIDITY 100] UIDs valid\r\n{tag} OK [READ-WRITE] SELECT completed\r\n"
        );
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. UID STORE 42 +FLAGS (\Seen)
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.contains("UID STORE 42 +FLAGS (\\Seen)"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* 1 FETCH (UID 42 FLAGS (\\Seen))\r\n{tag} OK STORE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let change = Change::Read { on: true };
    let result = apply_change(&mut session, &caps, &folders, &threads, &change).await;
    assert!(result.is_ok());
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_move_with_extension() {
    let (mut session, mut server_io) = setup_mock_session().await;
    let caps = ImapCapabilities {
        move_support: true,
        ..Default::default()
    };

    let folders = DiscoveredFolders::new(vec![RemoteFolder {
        remote_id: "Archive".to_string(),
        name: "Archive".to_string(),
        kind: FolderKind::Archive,
        parent: None,
    }]);

    let threads = vec![ThreadTarget {
        thread_id: "thread:1".to_string(),
        messages: vec![TargetMessage {
            id: "INBOX:100:42".to_string(),
            folder: Some(FolderKind::Inbox),
            unread: false,
            ..Default::default()
        }],
    }];

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // SELECT INBOX
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("SELECT"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("{tag} OK SELECT completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // UID MOVE 42 Archive
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID MOVE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("{tag} OK MOVE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let change = Change::Archive;
    let result = apply_change(&mut session, &caps, &folders, &threads, &change).await;
    assert!(result.is_ok());
    server_task.await.unwrap();
}

#[tokio::test]
async fn test_move_fallback_copy_delete_expunge() {
    let (mut session, mut server_io) = setup_mock_session().await;
    let caps = ImapCapabilities {
        move_support: false,
        ..Default::default()
    };

    let folders = DiscoveredFolders::new(vec![RemoteFolder {
        remote_id: "Trash".to_string(),
        name: "Trash".to_string(),
        kind: FolderKind::Trash,
        parent: None,
    }]);

    let threads = vec![ThreadTarget {
        thread_id: "thread:1".to_string(),
        messages: vec![TargetMessage {
            id: "INBOX:100:42".to_string(),
            folder: Some(FolderKind::Inbox),
            unread: false,
            ..Default::default()
        }],
    }];

    let server_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];

        // SELECT INBOX
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("SELECT"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("{tag} OK SELECT completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 1. UID COPY 42 Trash
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID COPY"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("{tag} OK COPY completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 2. UID STORE 42 +FLAGS (\Deleted)
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("UID STORE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* 1 FETCH (UID 42 FLAGS (\\Deleted))\r\n{tag} OK STORE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();

        // 3. EXPUNGE
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        assert!(cmd.to_uppercase().contains("EXPUNGE"));
        let tag = cmd.split_whitespace().next().unwrap();
        let resp = format!("* 1 EXPUNGE\r\n{tag} OK EXPUNGE completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    let change = Change::Trash;
    let result = apply_change(&mut session, &caps, &folders, &threads, &change).await;
    assert!(result.is_ok());
    server_task.await.unwrap();
}
