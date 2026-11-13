//! IMAP/SMTP accounts in MCP: account list, status, sync status, removal, and refusal of pasted passwords.

use serde_json::json;

use super::{Reply, call, host, start, structured, text, write_config};

const IMAP_CONFIG: &str = r#"[general]
secrets = "file"

[[accounts]]
name = "personal"
kind = "imapsmtp"
address = "user@example.com"
imap_host = "imap.example.com"
imap_port = 993
imap_user = "user@example.com"
smtp_host = "smtp.example.com"
smtp_port = 465
smtp_user = "user@example.com"
"#;

const CANARY_PW: &str = "CANARY-PASSWORD-9999";

#[tokio::test]
async fn imapsmtp_account_in_accounts_list_and_status() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, IMAP_CONFIG);
    let client = start(&dir, host(None), None).await;

    // 1. Without password stored: shows "no password (needs rail secrets password)"
    let listed = call(&client, "rail_accounts_list", json!({})).await;
    let t = text(&listed);
    assert!(
        t.contains(
            "personal <user@example.com> (imapsmtp): no password (needs rail secrets password)"
        ),
        "actual: {t}"
    );

    let status = call(&client, "rail_status", json!({})).await;
    let t_status = text(&status);
    assert!(
        t_status.contains(
            "personal <user@example.com> (imapsmtp): no password (needs rail secrets password)"
        ),
        "actual: {t_status}"
    );

    // 2. Set password in file secret store
    let secrets_file = dir.path().join("data").join("secrets.json");
    std::fs::create_dir_all(secrets_file.parent().unwrap()).unwrap();
    let json_content = serde_json::json!({
        "tokens": {},
        "passwords": {
            "pw:user@example.com": "secret123"
        }
    });
    std::fs::write(&secrets_file, serde_json::to_string(&json_content).unwrap()).unwrap();

    // 3. Now accounts_list and status show "logged in"
    let listed2 = call(&client, "rail_accounts_list", json!({})).await;
    let t2 = text(&listed2);
    assert!(
        t2.contains("personal <user@example.com> (imapsmtp): logged in"),
        "actual: {t2}"
    );

    let status2 = call(&client, "rail_status", json!({})).await;
    let t2_status = text(&status2);
    assert!(
        t2_status.contains("personal <user@example.com> (imapsmtp): logged in"),
        "actual: {t2_status}"
    );

    client.close().await;
}

#[tokio::test]
async fn imapsmtp_sync_status_and_sync_now() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, IMAP_CONFIG);

    // Seed password
    let secrets_file = dir.path().join("data").join("secrets.json");
    std::fs::create_dir_all(secrets_file.parent().unwrap()).unwrap();
    let json_content = serde_json::json!({
        "tokens": {},
        "passwords": {
            "pw:user@example.com": "secret123"
        }
    });
    std::fs::write(&secrets_file, serde_json::to_string(&json_content).unwrap()).unwrap();

    let client = start(&dir, host(None), None).await;

    let sync_stat = call(&client, "rail_sync_status", json!({})).await;
    let t = text(&sync_stat);
    assert!(t.contains("personal:"), "actual: {t}");

    let sync_now = call(&client, "rail_sync_now", json!({"account": "personal"})).await;
    assert_ne!(sync_now.is_error, Some(true), "{}", text(&sync_now));
    let data = structured(&sync_now);
    assert_eq!(data["targets"][0]["name"], "personal", "{data}");

    client.close().await;
}

#[tokio::test]
async fn imapsmtp_account_remove_with_elicitation() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, IMAP_CONFIG);

    // Seed password
    let secrets_file = dir.path().join("data").join("secrets.json");
    std::fs::create_dir_all(secrets_file.parent().unwrap()).unwrap();
    let json_content = serde_json::json!({
        "tokens": {},
        "passwords": {
            "pw:user@example.com": "secret123"
        }
    });
    std::fs::write(&secrets_file, serde_json::to_string(&json_content).unwrap()).unwrap();

    let client = start(&dir, host(Some(Reply::Yes)), None).await;
    let res = call(
        &client,
        "rail_account_remove",
        json!({"account": "personal"}),
    )
    .await;
    assert_ne!(res.is_error, Some(true), "{}", text(&res));
    let t = text(&res);
    assert!(
        t.contains("and its stored passwords were deleted"),
        "actual: {t}"
    );

    // Config no longer holds the account
    let cfg = std::fs::read_to_string(dir.path().join("config").join("config.toml")).unwrap();
    assert!(!cfg.contains("user@example.com"));

    // Password was deleted
    let secrets_after = std::fs::read_to_string(&secrets_file).unwrap();
    assert!(!secrets_after.contains("secret123"));

    client.close().await;
}

#[tokio::test]
async fn imapsmtp_account_add_and_login_refused_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, IMAP_CONFIG);
    let client = start(&dir, host(None), None).await;

    // 1. Refuse rail_account_add with pasted password and never echo canary
    let pasted_pw = json!({
        "kind": "imapsmtp",
        "address": "other@example.com",
        "password": CANARY_PW
    });
    let res_pw = call(&client, "rail_account_add", pasted_pw).await;
    assert_eq!(res_pw.is_error, Some(true));
    let t_pw = text(&res_pw);
    assert!(
        t_pw.contains("Rail does not take passwords from agents"),
        "actual: {t_pw}"
    );
    assert!(!t_pw.contains(CANARY_PW));

    // 2. Refuse rail_account_add with kind imapsmtp even without secret
    let imap_add = json!({
        "kind": "imapsmtp",
        "address": "other@example.com"
    });
    let res_imap = call(&client, "rail_account_add", imap_add).await;
    assert_eq!(res_imap.is_error, Some(true));
    let t_imap = text(&res_imap);
    assert!(
        t_imap.contains("IMAP/SMTP accounts do not use OAuth browser login"),
        "actual: {t_imap}"
    );

    // 3. Refuse rail_account_login for imapsmtp
    let res_login = call(
        &client,
        "rail_account_login",
        json!({"account": "personal"}),
    )
    .await;
    assert_eq!(res_login.is_error, Some(true));
    let t_login = text(&res_login);
    assert!(
        t_login.contains("IMAP/SMTP accounts do not use OAuth browser login"),
        "actual: {t_login}"
    );

    client.close().await;

    // Check log never holds the canary
    let log = std::fs::read_to_string(dir.path().join("state").join("rail.log")).unwrap();
    assert!(!log.contains(CANARY_PW));
}
