//! The send path of a `Send` row, and the one place that decides whether a message may go out
//! again. Before its send call the draft is stamped (`send_attempted_at`, kept through a
//! failure, a retry or a new `Send` row). A stamped draft, a row with an attempt counted, and a
//! row found in flight at start (the recovered set) all ask the provider first
//! (`Provider::sent_already`): sent means the row completes as if this send succeeded; not
//! sent means it sends, with the same `Message-ID`. `Provider::send` is never retried inside
//! the HTTP layer, and any 2xx answer to it is a send.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::PoisonError;

use super::Worker;
use super::compose::{self, Assembly};
use crate::ops::{DraftId, OutboxId};
use crate::paths::{import_dir_in, remove_dir_if_present};
use crate::provider::{DraftIds, ProviderError, SentReceipt};
use crate::store::drafts::ListedFile;
use crate::store::outbox::{OutboxOutcome, OutboxRow, OutboxWrite};

/// What a send or a push reads from the store in one go.
pub(super) struct Loaded {
    pub(super) send_attempted: bool,
    pub(super) attachments_readable: bool,
    /// The message with its provider copy and thread, and what is left to read.
    pub(super) assembly: Assembly,
}

impl Worker {
    /// The restart rule: the account's inflight rows back to pending, remembered so a `Send`
    /// among them is looked up before it is sent.
    pub(super) async fn recover(&self) {
        let recovered = self
            .writer
            .outbox(OutboxWrite::Recover {
                account_id: self.account_id,
            })
            .await;
        match recovered {
            Ok(OutboxOutcome::Recovered(ids)) => {
                self.recovered_ids().extend(ids);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(account = %self.name, error = %error.message, "could not recover the outbox");
            }
        }
    }

    fn recovered_ids(&self) -> std::sync::MutexGuard<'_, HashSet<OutboxId>> {
        self.recovered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The row is settled (sent, failed or back to pending with an attempt counted): the
    /// attempt count carries the rule from here.
    pub(super) fn forget_recovered(&self, id: OutboxId) {
        self.recovered_ids().remove(&id);
    }

    /// True when an earlier send of the draft may have reached Gmail.
    fn may_have_been_sent(&self, row: &OutboxRow, loaded: &Loaded) -> bool {
        loaded.send_attempted || row.attempts > 0 || self.recovered_ids().contains(&row.id)
    }

    /// Stamp the draft before its send call, with the provider's draft that call sends and
    /// the sizes it lists for the draft's files (Graph); no send without the stamp.
    async fn stamp(
        &self,
        draft: DraftId,
        remote: Option<DraftIds>,
        listed: Vec<ListedFile>,
    ) -> Result<(), ProviderError> {
        self.writer
            .outbox(OutboxWrite::DraftSendAttempted {
                draft,
                remote,
                listed,
            })
            .await
            .map(|_| ())
            .map_err(|e| ProviderError::transient(&e.message))
    }

    /// Send the draft of a `Send` row, unless the provider already holds it as sent. A
    /// provider that sends its own draft (Graph) gets it saved first, and its id is kept
    /// before the send: a send cut off from there finds that draft instead of making another.
    pub(super) async fn send_draft(
        &self,
        row: &OutboxRow,
        draft: DraftId,
    ) -> Result<(), ProviderError> {
        let loaded = self.load(draft).await?;
        if self.may_have_been_sent(row, &loaded)
            && self.provider.sent_already(&loaded.assembly.mail).await?
        {
            tracing::info!(account = %self.name, row = row.id, "the message was sent already");
            return Ok(());
        }
        if !loaded.attachments_readable {
            return Err(ProviderError::permanent(
                "the draft's attachments could not be read",
            ));
        }
        let paths = compose::file_paths(&loaded.assembly);
        let mut mail = compose::attach(
            loaded.assembly,
            self.provider.as_ref(),
            self.kind.provider_name(),
        )
        .await?;
        let (mut remote, mut listed) = (None, Vec::new());
        if self.provider.send_consumes_draft() {
            let saved = self.provider.draft_save(&mail).await?;
            listed = compose::listed_files(&paths, &mail, &saved.listed);
            compose::with_listed(&mut mail, &saved.listed);
            mail.remote = Some(saved.ids.clone());
            remote = Some(saved.ids);
        }
        self.stamp(draft, remote, listed).await?;
        if self.provider.two_phase_send() {
            let receipt = if row.relay_accepted_at.is_some() {
                SentReceipt {
                    message_id: Some(mail.outgoing.message_id.clone()),
                    thread_id: None,
                }
            } else {
                let receipt = self.provider.relay_send(&mail).await?;
                // The relay accepted it: record that before touching the Sent copy, so a
                // restart or a retry never submits the message a second time.
                self.record_relay_accepted(row.id).await?;
                receipt
            };
            self.provider.finish_send(&mail, receipt).await.map(|_| ())
        } else {
            self.provider.send(&mail).await.map(|_| ())
        }
    }

    /// Persist relay acceptance, retrying only this write: the message has left Rail and must
    /// never be submitted again, so giving up here would risk a duplicate.
    async fn record_relay_accepted(&self, id: OutboxId) -> Result<(), ProviderError> {
        let mut wait = super::MIN_IDLE;
        loop {
            match self.writer.outbox(OutboxWrite::RelayAccepted { id }).await {
                Ok(_) => return Ok(()),
                Err(error) if self.writer.is_closed() => {
                    return Err(ProviderError::transient(&error.message));
                }
                Err(error) => {
                    tracing::warn!(
                        account = %self.name,
                        row = id,
                        error = %error.message,
                        retry_secs = wait.as_secs(),
                        "could not record relay acceptance; retrying the write"
                    );
                    tokio::time::sleep(wait).await;
                    wait = wait.saturating_mul(2).min(super::MAX_IDLE);
                }
            }
        }
    }

    /// The import directory the draft owns: the one its own client token names under the draft
    /// files root, when it exists. None for a draft composed in Rail, one that is gone, and a
    /// token that is not one plain path segment. A path stored in the draft is never used.
    pub(super) async fn import_dir_of(&self, draft: DraftId) -> Option<PathBuf> {
        let root = self.draft_files_dir.clone();
        let found = self
            .read(move |store| {
                Ok(store
                    .draft_client_token(draft)?
                    .and_then(|token| import_dir_in(&root, &token))
                    .filter(|dir| dir.is_dir()))
            })
            .await;
        found.unwrap_or_else(|e| {
            tracing::warn!(draft, code = ?e.code, "a sent draft's files could not be looked up");
            None
        })
    }

    pub(super) async fn load(&self, draft: DraftId) -> Result<Loaded, ProviderError> {
        let (account, from, from_name) = (
            self.account_id,
            self.address.clone(),
            self.from_name.clone(),
        );
        let loaded = self
            .read(move |store| {
                let Some(d) = store.draft(draft)?.filter(|d| d.account_id == account) else {
                    return Ok(None);
                };
                let attachments_readable = store.draft_attachments_readable(draft)?;
                let assembly = compose::outgoing_for(store, &d, &from, from_name.as_deref())?;
                Ok(Some(Loaded {
                    send_attempted: d.send_attempted_at.is_some(),
                    attachments_readable,
                    assembly,
                }))
            })
            .await
            .map_err(|e| ProviderError::transient(&e.message))?;
        loaded.ok_or_else(|| ProviderError::permanent("the draft is gone"))
    }
}

/// Remove a sent draft's import directory. Best effort: the send is done, and the sweep at the
/// next daemon start takes what is left. A failure is logged by draft id, never the path.
pub(super) async fn remove_import_dir(draft: DraftId, dir: PathBuf) {
    let removed = tokio::task::spawn_blocking(move || remove_dir_if_present(&dir)).await;
    match removed {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            tracing::warn!(draft, error_kind = ?e.kind(), "a sent draft's files could not be removed");
        }
        Err(_) => tracing::warn!(draft, "removing a sent draft's files did not run"),
    }
}
