//! A successful token exchange must not bind a different mailbox to an account.

use super::*;
use crate::auth::TokenSet;
use crate::error::ErrorClass;

pub(super) async fn mount_matching_identity(server: &MockServer) {
    for (at, body) in [
        (
            "/gmail/v1/users/me/profile",
            json!({"emailAddress": "me@example.com"}),
        ),
        (
            "/v1.0/me",
            json!({"mail": "me@example.com", "userPrincipalName": "me@example.com"}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .with_priority(5)
            .mount(server)
            .await;
    }
}

async fn wrong_identity(h: &Harness, kind: AccountKind) {
    let (at, body) = match kind {
        AccountKind::Gmail => (
            "/gmail/v1/users/me/profile",
            json!({"emailAddress": "personal@example.com"}),
        ),
        AccountKind::M365 => (
            "/v1.0/me",
            json!({"mail": "personal@example.com", "userPrincipalName": "personal@example.com"}),
        ),
        AccountKind::ImapSmtp => panic!("IMAP/SMTP does not use REST identity mocks"),
    };
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .with_priority(1)
        .mount(&h._token_server)
        .await;
}

fn assert_wrong_mailbox(state: PendingState) {
    let PendingState::Failed { error } = state else {
        panic!("a different mailbox must fail login: {state:?}");
    };
    assert_eq!(error.code, ErrorCode::Auth);
    assert_eq!(error.class, ErrorClass::Permanent);
    assert!(error.message.contains("mailbox"), "{error:?}");
}

async fn rejected_relogin(kind: AccountKind) {
    let config = WORK.replace("kind = \"gmail\"", &format!("kind = \"{}\"", kind.as_str()));
    let mut h = harness(&config).await;
    let secrets = FileSecretStore::new(h.paths.secrets_file());
    let old = TokenSet {
        access_token: "old-access".into(),
        refresh_token: "old-refresh".into(),
        expires_at: i64::MAX / 2,
    };
    secrets.set_tokens("me@example.com", &old).unwrap();
    wrong_identity(&h, kind).await;
    let request = LoginBeginRequest {
        account: "work".into(),
        open_browser: true,
    };
    let begun = h.client.call::<AccountLoginBegin>(&request).await.unwrap();
    browser(&mut h.urls, &format!("code={CODE}")).await;
    assert_wrong_mailbox(wait(&h.client, &begun.id, 10).await.state);
    assert_eq!(secrets.get_tokens("me@example.com").unwrap(), Some(old));
    assert_eq!(
        std::fs::read_to_string(h.paths.config_file()).unwrap(),
        config
    );
}

async fn rejected_add(kind: AccountKind) {
    let mut h = harness(FILE_BACKEND).await;
    wrong_identity(&h, kind).await;
    let mut request = add_request("me@example.com");
    request.kind = kind;
    let begun = h.client.call::<AccountAddBegin>(&request).await.unwrap();
    browser(&mut h.urls, &format!("code={CODE}")).await;
    assert_wrong_mailbox(wait(&h.client, &begun.id, 10).await.state);
    assert!(!h.paths.secrets_file().exists());
    assert_eq!(
        std::fs::read_to_string(h.paths.config_file()).unwrap(),
        FILE_BACKEND
    );
    assert!(
        Store::open(&h.paths.db_file())
            .unwrap()
            .list_accounts()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_different_gmail_mailbox_does_not_replace_existing_tokens() {
    rejected_relogin(AccountKind::Gmail).await;
}

#[tokio::test]
async fn a_different_outlook_mailbox_does_not_replace_existing_tokens() {
    rejected_relogin(AccountKind::M365).await;
}

#[tokio::test]
async fn a_different_gmail_mailbox_is_not_added_to_config_or_secrets() {
    rejected_add(AccountKind::Gmail).await;
}

#[tokio::test]
async fn a_different_outlook_mailbox_is_not_added_to_config_or_secrets() {
    rejected_add(AccountKind::M365).await;
}

#[tokio::test]
async fn a_failed_identity_check_does_not_save_new_tokens() {
    for response in [
        ResponseTemplate::new(500),
        ResponseTemplate::new(200).set_body_string("not json"),
    ] {
        let mut h = harness(FILE_BACKEND).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/profile"))
            .respond_with(response)
            .with_priority(1)
            .mount(&h._token_server)
            .await;
        let begun = h
            .client
            .call::<AccountAddBegin>(&add_request("me@example.com"))
            .await
            .unwrap();
        browser(&mut h.urls, &format!("code={CODE}")).await;
        assert!(matches!(
            wait(&h.client, &begun.id, 10).await.state,
            PendingState::Failed { .. }
        ));
        assert!(!h.paths.secrets_file().exists());
        assert!(load(&h.paths.config_file()).unwrap().accounts.is_empty());
    }
}
