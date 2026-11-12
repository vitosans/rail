use std::io::{Error as IoError, ErrorKind};

use async_imap::error::Error as ImapError;

use super::client::{ImapCapabilities, ImapStream, classify_imap_error, parse_server_name};
use crate::error::ErrorClass;
use crate::ops::Secret;
use crate::provider::ProviderError;

#[test]
fn test_imap_capabilities_detection() {
    let detected = ImapCapabilities::from_strs(["MOVE", "UIDPLUS", "IDLE", "STARTTLS"]);
    assert!(detected.move_support);
    assert!(detected.uidplus_support);
    assert!(detected.idle_support);
    assert!(detected.starttls_support);

    let empty = ImapCapabilities::from_strs(Vec::<&str>::new());
    assert!(!empty.move_support);
    assert!(!empty.uidplus_support);
    assert!(!empty.idle_support);
    assert!(!empty.starttls_support);
}

#[test]
fn test_classify_imap_io_error() {
    let err = ImapError::Io(IoError::new(ErrorKind::ConnectionReset, "connection reset"));
    let classified = classify_imap_error(&err);
    assert_eq!(classified.class(), ErrorClass::Transient);
    assert_eq!(classified.code(), "transient");

    let lost = ImapError::ConnectionLost;
    let classified_lost = classify_imap_error(&lost);
    assert_eq!(classified_lost.class(), ErrorClass::Transient);
}

#[test]
fn test_classify_imap_auth_failure() {
    let err = ImapError::No("[AUTHENTICATIONFAILED] Invalid credentials".to_string());
    let classified = classify_imap_error(&err);
    assert_eq!(classified.class(), ErrorClass::Permanent);
    assert_eq!(classified.code(), "needs_auth");
    assert!(matches!(classified, ProviderError::NeedsAuth { .. }));

    let err2 = ImapError::Bad("Login failed: bad username or password".to_string());
    let classified2 = classify_imap_error(&err2);
    assert_eq!(classified2.class(), ErrorClass::Permanent);
    assert_eq!(classified2.code(), "needs_auth");
}

#[test]
fn test_classify_imap_general_error() {
    let err = ImapError::No("Mailbox does not exist".to_string());
    let classified = classify_imap_error(&err);
    assert_eq!(classified.class(), ErrorClass::Permanent);
    assert_eq!(classified.code(), "permanent");

    let err_append = ImapError::Append;
    let classified_append = classify_imap_error(&err_append);
    assert_eq!(classified_append.class(), ErrorClass::Transient);
}

#[test]
fn test_parse_server_name() {
    assert!(parse_server_name("imap.example.com").is_ok());
    assert!(parse_server_name("127.0.0.1").is_ok());
    assert!(parse_server_name("").is_err());
}

#[tokio::test]
async fn test_mock_stream_greeting_and_login() {
    let (client_io, mut server_io) = tokio::io::duplex(4096);
    let mut client = async_imap::Client::new(ImapStream::Mock(client_io));

    // Spawn mock server interaction
    let server_handle = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Send server greeting
        server_io
            .write_all(b"* OK [CAPABILITY IMAP4rev1 IDLE MOVE UIDPLUS] IMAP4rev1 Service Ready\r\n")
            .await
            .unwrap();

        // Read login command
        let mut buf = vec![0u8; 1024];
        let n = server_io.read(&mut buf).await.unwrap();
        let cmd = String::from_utf8_lossy(&buf[..n]);
        // Extract tag from cmd: e.g. "a0000 LOGIN ..."
        let tag = cmd.split_whitespace().next().unwrap_or("a0000");

        // Respond with login OK with matching tag
        let resp = format!("{tag} OK [CAPABILITY IMAP4rev1 IDLE MOVE UIDPLUS] LOGIN completed\r\n");
        server_io.write_all(resp.as_bytes()).await.unwrap();
    });

    // Client reads greeting
    let greeting = client.read_response().await.unwrap();
    assert!(greeting.is_some());

    // Client logs in
    let password = Secret::new("s3cr3t_p@ss".to_string());
    let (session, caps) = client
        .login_with_capabilities("user@example.com", password.expose())
        .await
        .map_err(|(e, _)| classify_imap_error(&e))
        .unwrap();

    let resolved_caps = match caps {
        Some(c) => ImapCapabilities::from_capabilities(&c),
        None => ImapCapabilities::default(),
    };
    assert!(resolved_caps.move_support);
    assert!(resolved_caps.uidplus_support);
    assert!(resolved_caps.idle_support);

    server_handle.await.unwrap();
    let _ = session;
}
