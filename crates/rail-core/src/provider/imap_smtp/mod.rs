//! Generic IMAP and SMTP provider.

use async_imap::Session;

pub mod actions;
#[cfg(test)]
mod actions_tests;
pub mod client;
#[cfg(test)]
mod client_tests;
pub mod drafts;
#[cfg(test)]
mod drafts_tests;
pub mod folders;
#[cfg(test)]
mod folders_tests;
pub mod identity;
#[cfg(test)]
mod identity_tests;
pub mod read;
#[cfg(test)]
mod read_tests;
pub mod send;
#[cfg(test)]
mod send_tests;
pub mod smtp;
#[cfg(test)]
mod smtp_tests;
pub mod sync;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use crate::config::model::TlsMode;
use crate::ops::Secret;
use crate::provider::{
    Body, Change, ChangeSet, EnvelopePage, IdPage, OutgoingMail, PageToken, Profile, Provider,
    ProviderError, RemoteFolder, RemoteId, SavedDraft, SentReceipt, SyncCursor, ThreadTarget,
    Window,
};
use client::{ImapCapabilities, ImapStream, connect_and_login_imap};
use folders::DiscoveredFolders;
use smtp::{SmtpTransport, build_smtp_transport};

/// A mail provider that reads via IMAP and sends via SMTP.
#[derive(Clone)]
pub struct ImapSmtpProvider {
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls_mode: TlsMode,
    pub username: String,
    pub password: Secret<String>,
    pub smtp_host: String,
    pub smtp_port: Option<u16>,
    pub smtp_tls_mode: TlsMode,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<Secret<String>>,
    pub sent_mailbox_name: Option<String>,
    /// Whether a sent message is appended to the Sent mailbox. False for servers such as
    /// Gmail and iCloud that file their own copy of submitted mail.
    pub append_to_sent: bool,
    pub relay_accepted: Arc<Mutex<HashSet<String>>>,
}

impl ImapSmtpProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        imap_host: String,
        imap_port: u16,
        imap_tls_mode: TlsMode,
        username: String,
        password: Secret<String>,
        smtp_host: String,
        smtp_port: Option<u16>,
        smtp_tls_mode: TlsMode,
        smtp_username: Option<String>,
        smtp_password: Option<Secret<String>>,
        sent_mailbox_name: Option<String>,
    ) -> Self {
        Self {
            imap_host,
            imap_port,
            imap_tls_mode,
            username,
            password,
            smtp_host,
            smtp_port,
            smtp_tls_mode,
            smtp_username,
            smtp_password,
            sent_mailbox_name,
            append_to_sent: true,
            relay_accepted: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn is_relay_accepted(&self, message_id: &str) -> bool {
        self.relay_accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(message_id)
    }

    pub fn mark_relay_accepted(&self, message_id: &str) {
        self.relay_accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message_id.to_string());
    }

    async fn connect_imap(&self) -> Result<(Session<ImapStream>, ImapCapabilities), ProviderError> {
        connect_and_login_imap(
            &self.imap_host,
            self.imap_port,
            self.imap_tls_mode,
            &self.username,
            &self.password,
        )
        .await
    }

    fn smtp_transport(&self) -> Result<SmtpTransport, ProviderError> {
        build_smtp_transport(
            &self.smtp_host,
            self.smtp_port,
            self.smtp_tls_mode,
            self.smtp_username.as_deref(),
            self.smtp_password.as_ref(),
        )
    }

    async fn discover_folders(
        &self,
        session: &mut Session<ImapStream>,
    ) -> Result<DiscoveredFolders, ProviderError> {
        folders::discover_folders(session, self.sent_mailbox_name.as_deref()).await
    }
}

#[async_trait::async_trait]
impl Provider for ImapSmtpProvider {
    async fn folders(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        let discovered = self.discover_folders(&mut session).await?;
        Ok(discovered.folders)
    }

    async fn profile(&self) -> Result<Profile, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        let status = session
            .examine("INBOX")
            .await
            .map_err(|e| client::classify_imap_error(&e))?;
        let uidvalidity = status.uid_validity.unwrap_or(0);
        let exists = status.exists as u64;
        let last_uid = status.uid_next.unwrap_or(1).saturating_sub(1);
        let cursor = sync::ImapSyncCursor::new("INBOX", uidvalidity, last_uid).to_sync_cursor();
        Ok(Profile {
            cursor,
            messages_total: Some(exists),
        })
    }

    async fn list(
        &self,
        window: &Window,
        page: Option<&PageToken>,
    ) -> Result<IdPage, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        read::list_messages(
            &mut session,
            "INBOX",
            0,
            window,
            page,
            self.list_page_size(),
        )
        .await
    }

    fn list_page_size(&self) -> usize {
        100
    }

    fn backfills_by_delta(&self) -> bool {
        false
    }

    async fn changes(&self, cursor: &SyncCursor) -> Result<ChangeSet, ProviderError> {
        if sync::ImapSyncCursor::parse(cursor).is_none() {
            return Err(ProviderError::CursorExpired);
        }
        let (mut session, _) = self.connect_imap().await?;
        sync::sync_changes(&mut session, cursor, self.list_page_size()).await
    }

    async fn body(&self, id: &str) -> Result<Body, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        read::fetch_body(&mut session, &id.to_string()).await
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<RemoteId>, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        read::search_messages(&mut session, "INBOX", 0, query, limit).await
    }

    async fn envelopes(&self, ids: &[RemoteId]) -> Result<EnvelopePage, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        read::fetch_envelopes(&mut session, ids).await
    }

    async fn apply(&self, threads: &[ThreadTarget], change: &Change) -> Result<(), ProviderError> {
        self.apply_with_moves(threads, change).await.map(|_| ())
    }

    async fn apply_with_moves(
        &self,
        threads: &[ThreadTarget],
        change: &Change,
    ) -> Result<Vec<(RemoteId, RemoteId)>, ProviderError> {
        let (mut session, caps) = self.connect_imap().await?;
        let discovered = self.discover_folders(&mut session).await?;
        actions::apply_change(&mut session, &caps, &discovered, threads, change).await
    }

    async fn send(&self, mail: &OutgoingMail) -> Result<SentReceipt, ProviderError> {
        let receipt = if self.is_relay_accepted(&mail.outgoing.message_id) {
            SentReceipt {
                message_id: Some(mail.outgoing.message_id.clone()),
                thread_id: None,
            }
        } else {
            let receipt = self.relay_send(mail).await?;
            self.mark_relay_accepted(&mail.outgoing.message_id);
            receipt
        };
        self.finish_send(mail, receipt).await
    }

    fn two_phase_send(&self) -> bool {
        true
    }

    async fn relay_send(&self, mail: &OutgoingMail) -> Result<SentReceipt, ProviderError> {
        let msg_id = &mail.outgoing.message_id;
        if self.is_relay_accepted(msg_id) {
            return Ok(SentReceipt {
                message_id: Some(msg_id.clone()),
                thread_id: None,
            });
        }
        let transport = self.smtp_transport()?;
        let receipt = smtp::send_smtp(&transport, mail).await?;
        self.mark_relay_accepted(msg_id);
        Ok(receipt)
    }

    async fn finish_send(
        &self,
        mail: &OutgoingMail,
        receipt: SentReceipt,
    ) -> Result<SentReceipt, ProviderError> {
        let msg_id = &mail.outgoing.message_id;
        if !self.append_to_sent {
            self.relay_accepted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(msg_id);
            return Ok(receipt);
        }
        let appended = async {
            let (mut session, _) = self.connect_imap().await?;
            let discovered = self.discover_folders(&mut session).await?;
            let sent_mailbox = discovered.sent_mailbox_name().to_string();
            send::append_to_sent(&mut session, &sent_mailbox, mail).await
        }
        .await;
        self.relay_accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(msg_id);
        match appended {
            Ok(sent_id) => {
                let mut final_receipt = receipt;
                if sent_id.is_some() {
                    final_receipt.message_id = sent_id;
                }
                Ok(final_receipt)
            }
            Err(err) => {
                tracing::warn!(
                    message_id = %msg_id,
                    error = %err,
                    "SMTP relay accepted message, but IMAP Sent append failed"
                );
                // SMTP has accepted: this is not a send failure. The recipient has the mail.
                Ok(receipt)
            }
        }
    }

    async fn draft_save(&self, mail: &OutgoingMail) -> Result<SavedDraft, ProviderError> {
        let (mut session, caps) = self.connect_imap().await?;
        let discovered = self.discover_folders(&mut session).await?;
        let drafts_mailbox = discovered.drafts_mailbox_name();
        drafts::save_draft(&mut session, &caps, drafts_mailbox, mail).await
    }

    async fn sent_already(&self, mail: &OutgoingMail) -> Result<bool, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        let discovered = self.discover_folders(&mut session).await?;
        let sent_mailbox = discovered.sent_mailbox_name();
        send::check_sent_already(&mut session, sent_mailbox, &mail.outgoing.message_id).await
    }

    fn send_consumes_draft(&self) -> bool {
        false
    }

    fn forward_carries_originals(&self) -> bool {
        false
    }

    async fn attachment(&self, message: &RemoteId, part: &str) -> Result<Vec<u8>, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        read::fetch_attachment(&mut session, message, part).await
    }

    async fn draft_delete(&self, draft_id: &str) -> Result<(), ProviderError> {
        let (mut session, caps) = self.connect_imap().await?;
        drafts::delete_draft(&mut session, &caps, draft_id).await
    }

    async fn draft_find(&self, message_id: &str) -> Result<Option<RemoteId>, ProviderError> {
        let (mut session, _) = self.connect_imap().await?;
        let discovered = self.discover_folders(&mut session).await?;
        let drafts_mailbox = discovered.drafts_mailbox_name();
        drafts::find_draft(&mut session, drafts_mailbox, message_id).await
    }
}
