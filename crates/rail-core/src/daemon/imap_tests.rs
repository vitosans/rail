//! Wire tests for `account.validate` and password management operations.

use std::sync::Arc;

use super::client::DaemonClient;
use super::{Connector, ServeConfig};
use crate::ops::op::{
    AccountPasswordCheck, AccountPasswordDelete, AccountPasswordSet, AccountValidate,
};
use crate::ops::{
    AccountPasswordCheckRequest, AccountPasswordDeleteRequest, AccountPasswordSetRequest,
    AccountValidateRequest, Secret,
};
use crate::paths::Paths;
use crate::test_support::{daemon_context, start_daemon};

const IMAP_CONFIG: &str = r#"
[general]
secrets = "file"

[[accounts]]
name = "mail"
kind = "imapsmtp"
address = "user@example.com"
imap_host = "imap.example.com"
imap_port = 993
imap_tls = "required"
smtp_host = "smtp.example.com"
smtp_port = 465
smtp_tls = "required"
"#;

async fn test_daemon(tmp: &tempfile::TempDir, config: &str) -> (Paths, DaemonClient) {
    let paths = Paths::all_in(tmp.path());
    crate::paths::create_private_dir(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), config).unwrap();
    let (ctx, _) = daemon_context(&paths);
    let _daemon = start_daemon(ctx, ServeConfig::default()).await;
    let client = Connector::new(paths.clone(), Arc::new(|| Ok(())))
        .connect()
        .await
        .unwrap();
    (paths, client)
}

#[tokio::test]
async fn test_password_operations_over_daemon_wire() {
    let tmp = tempfile::tempdir().unwrap();
    let (_paths, client) = test_daemon(&tmp, IMAP_CONFIG).await;

    // Check before setting: no password
    let check_before = client
        .call::<AccountPasswordCheck>(&AccountPasswordCheckRequest {
            account: "mail".into(),
            service: None,
        })
        .await
        .unwrap();
    assert!(!check_before.has_password);

    // Set password
    let set_res = client
        .call::<AccountPasswordSet>(&AccountPasswordSetRequest {
            account: "mail".into(),
            password: Secret::new("secret123".to_string()),
            service: None,
        })
        .await
        .unwrap();
    assert_eq!(set_res.account, "mail");

    // Check after setting: has password
    let check_after = client
        .call::<AccountPasswordCheck>(&AccountPasswordCheckRequest {
            account: "mail".into(),
            service: None,
        })
        .await
        .unwrap();
    assert!(check_after.has_password);

    // Delete password
    let del_res = client
        .call::<AccountPasswordDelete>(&AccountPasswordDeleteRequest {
            account: "mail".into(),
            service: None,
        })
        .await
        .unwrap();
    assert!(del_res.deleted);

    // Check after delete: no password
    let check_final = client
        .call::<AccountPasswordCheck>(&AccountPasswordCheckRequest {
            account: "mail".into(),
            service: None,
        })
        .await
        .unwrap();
    assert!(!check_final.has_password);
}

#[tokio::test]
async fn test_account_validate_wire_failure_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let (_paths, client) = test_daemon(&tmp, IMAP_CONFIG).await;

    // Validate with dummy host that fails to connect
    let err = client
        .call::<AccountValidate>(&AccountValidateRequest {
            account: Some("mail".into()),
            imap_host: None,
            imap_port: None,
            imap_tls: None,
            username: None,
            password: Some(Secret::new("testpass".to_string())),
            smtp_host: None,
            smtp_port: None,
            smtp_tls: None,
            smtp_username: None,
            smtp_password: None,
        })
        .await
        .unwrap_err();

    // Verify error was returned over wire
    assert!(!err.message.is_empty());
}
