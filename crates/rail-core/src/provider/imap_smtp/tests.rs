use super::*;
use crate::config::model::TlsMode;
use crate::ops::Secret;

fn sample_provider() -> ImapSmtpProvider {
    ImapSmtpProvider::new(
        "imap.example.com".into(),
        993,
        TlsMode::Required,
        "user@example.com".into(),
        Secret::new("password123".to_string()),
        "smtp.example.com".into(),
        Some(465),
        TlsMode::Required,
        Some("user@example.com".into()),
        Some(Secret::new("password123".to_string())),
        Some("Sent Items".into()),
    )
}

#[test]
fn test_imap_smtp_provider_properties() {
    let provider = sample_provider();
    assert_eq!(provider.list_page_size(), 100);
    assert!(!provider.backfills_by_delta());
    assert!(!provider.send_consumes_draft());
    assert!(!provider.forward_carries_originals());
    assert!(provider.standing_error().is_none());
    assert!(provider.walked_everything(&SyncCursor("cursor".into())));
}

#[tokio::test]
async fn test_changes_with_invalid_cursor_returns_cursor_expired() {
    let provider = sample_provider();
    let err = provider
        .changes(&SyncCursor("invalid_cursor".into()))
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::CursorExpired));
}

#[test]
fn test_relay_accepted_tracking() {
    let provider = sample_provider();
    let msg_id = "test-msg-123@example.com";
    assert!(!provider.is_relay_accepted(msg_id));
    provider.mark_relay_accepted(msg_id);
    assert!(provider.is_relay_accepted(msg_id));
}
