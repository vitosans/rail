//! Outbox worker tests for ImapSmtp two-phase send and Sent-folder retry logic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio::sync::Notify;

use super::sending_tests::{Harness, queued_draft};
use crate::provider::{
    Body, Change, ChangeSet, EnvelopePage, IdPage, OutgoingMail, PageToken, Profile, Provider,
    ProviderError, RemoteFolder, RemoteId, SavedDraft, SentReceipt, SyncCursor, ThreadTarget,
    Window,
};
use crate::store::outbox::OutboxState;

/// Mock provider that tracks SMTP submission count and simulates Sent append failures.
#[derive(Clone)]
struct MockTwoPhaseProvider {
    smtp_sends: Arc<AtomicUsize>,
    sent_appends: Arc<AtomicUsize>,
    fail_sent_append: Arc<AtomicBool>,
    relay_accepted: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl MockTwoPhaseProvider {
    fn new() -> Self {
        Self {
            smtp_sends: Arc::new(AtomicUsize::new(0)),
            sent_appends: Arc::new(AtomicUsize::new(0)),
            fail_sent_append: Arc::new(AtomicBool::new(false)),
            relay_accepted: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }
}

#[async_trait]
impl Provider for MockTwoPhaseProvider {
    async fn folders(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        Ok(Vec::new())
    }

    async fn profile(&self) -> Result<Profile, ProviderError> {
        Ok(Profile {
            cursor: SyncCursor("test".into()),
            messages_total: Some(0),
        })
    }

    async fn list(
        &self,
        _window: &Window,
        _page: Option<&PageToken>,
    ) -> Result<IdPage, ProviderError> {
        Ok(IdPage {
            ids: Vec::new(),
            next: None,
        })
    }

    fn list_page_size(&self) -> usize {
        100
    }

    async fn changes(&self, _cursor: &SyncCursor) -> Result<ChangeSet, ProviderError> {
        Ok(ChangeSet::default())
    }

    async fn body(&self, _id: &str) -> Result<Body, ProviderError> {
        Err(ProviderError::NotFound)
    }

    async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<RemoteId>, ProviderError> {
        Ok(Vec::new())
    }

    async fn envelopes(&self, _ids: &[RemoteId]) -> Result<EnvelopePage, ProviderError> {
        Ok(EnvelopePage::default())
    }

    async fn apply(
        &self,
        _threads: &[ThreadTarget],
        _change: &Change,
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn send(&self, mail: &OutgoingMail) -> Result<SentReceipt, ProviderError> {
        let msg_id = &mail.outgoing.message_id;
        let relay_done = self.relay_accepted.lock().unwrap().contains(msg_id);

        if !relay_done {
            self.smtp_sends.fetch_add(1, Ordering::SeqCst);
            self.relay_accepted.lock().unwrap().insert(msg_id.clone());
        }

        self.sent_appends.fetch_add(1, Ordering::SeqCst);
        if self.fail_sent_append.load(Ordering::SeqCst) {
            return Err(ProviderError::transient(
                "IMAP server full or append failed",
            ));
        }

        self.relay_accepted.lock().unwrap().remove(msg_id);
        Ok(SentReceipt {
            message_id: Some(msg_id.clone()),
            thread_id: None,
        })
    }

    async fn draft_save(&self, _mail: &OutgoingMail) -> Result<SavedDraft, ProviderError> {
        Err(ProviderError::NotFound)
    }

    async fn sent_already(&self, _mail: &OutgoingMail) -> Result<bool, ProviderError> {
        Ok(false)
    }

    fn send_consumes_draft(&self) -> bool {
        false
    }

    fn forward_carries_originals(&self) -> bool {
        false
    }

    async fn attachment(&self, _message: &RemoteId, _part: &str) -> Result<Vec<u8>, ProviderError> {
        Err(ProviderError::NotFound)
    }

    async fn draft_delete(&self, _draft_id: &str) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn draft_find(&self, _message_id: &str) -> Result<Option<RemoteId>, ProviderError> {
        Ok(None)
    }
}

#[tokio::test]
async fn test_outbox_two_phase_send_retries_sent_file_without_resending_smtp() {
    let h = Harness::new().await;
    let provider = MockTwoPhaseProvider::new();
    provider.fail_sent_append.store(true, Ordering::SeqCst);

    let (draft, row) = queued_draft(&h, 42, vec![]).await;
    let worker = h
        .rig
        .worker_on(Arc::new(provider.clone()), None, Arc::new(Notify::new()));

    // First attempt: SMTP succeeds (1 send), but IMAP Sent append fails
    worker.step().await;

    assert_eq!(provider.smtp_sends.load(Ordering::SeqCst), 1);
    assert_eq!(provider.sent_appends.load(Ordering::SeqCst), 1);

    let pending = h.row(row).await.unwrap();
    assert_eq!(pending.state, OutboxState::Pending);
    assert_eq!(pending.attempts, 1);
    assert!(h.with_store(|s| s.draft(draft).unwrap()).is_some());

    // Second attempt: Sent append now succeeds; SMTP MUST NOT be invoked again!
    provider.fail_sent_append.store(false, Ordering::SeqCst);
    h.advance_past(pending.not_before).await;
    worker.step().await;

    // Verify SMTP was still called ONLY ONCE in total!
    assert_eq!(
        provider.smtp_sends.load(Ordering::SeqCst),
        1,
        "SMTP submission must never be retried after relay acceptance"
    );
    // Sent append was retried and succeeded (total 2 calls)
    assert_eq!(provider.sent_appends.load(Ordering::SeqCst), 2);

    // Row and draft are now completed and cleaned up
    assert!(h.row(row).await.is_none());
    assert!(h.with_store(|s| s.draft(draft).unwrap()).is_none());
}
