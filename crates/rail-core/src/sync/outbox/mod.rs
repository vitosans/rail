//! One account's outbox worker, run by `SyncManager` beside the engine: it claims the oldest
//! due row, sends it through the account's provider (outside the engine's retry status), and
//! completes or fails it through the writer. It sleeps until the earliest `not_before` (at
//! least `MIN_IDLE`, at most `MAX_IDLE`) or the new-row signal; it pauses while the account
//! needs a login; stopping drops a call in flight (the row stays inflight, and the restart rule
//! sends it again), except a draft being sent or pushed to Gmail, which it lets finish (and
//! records) within the manager's stop deadline, so a create is never cut off before its ids are
//! kept. A `Send` row goes through `sending` (the never-twice rule); a `PushDraft` or
//! `DeleteDraft` row goes through `pushing`. Once a draft row completes, the account's sync
//! engine is nudged so Gmail's copy (Sent or Drafts) arrives. Snoozes that fall due go back on its
//! next pass, paused or not: the store queues their return and the status board tells who
//! watches; a wake the store fails is tried again after a wait that doubles up to `MAX_IDLE`.
//! Logs carry row ids, counts and codes only.

mod compose;
#[cfg(test)]
mod compose_tests;
#[cfg(test)]
mod gmail_send_tests;
#[cfg(test)]
mod graph_send_tests;
#[cfg(test)]
mod imap_send_tests;
mod plan;
#[cfg(test)]
mod plan_tests;
mod pushing;
#[cfg(test)]
mod pushing_tests;
#[cfg(test)]
mod return_tests;
mod sending;
#[cfg(test)]
mod sending_tests;
#[cfg(test)]
mod settlement_tests;
#[cfg(test)]
mod snooze_tests;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{Notify, watch};

use self::plan::Plan;
use self::sending::remove_import_dir;
use super::batch::{Batch, StateUpdate};
use super::engine::{Clock, stopped};
use super::manager::{retry_delay, unobserved};
use super::status::StatusBoard;
use super::writer::WriterHandle;
use crate::config::{AccountKind, FolderModel};
use crate::error::ErrorClass;
use crate::ops::{
    Action, ActionFailed, DraftId, ErrorCode, OpError, OutboxDepth, OutboxId, SnoozeWoke, SyncPhase,
};
use crate::provider::{Change, DraftIds, Provider, ProviderError, ThreadTarget};
use crate::store::drafts::ListedFile;
use crate::store::outbox::{Failure, OutboxOutcome, OutboxRow, OutboxWrite};
use crate::store::{Store, StoreError};

/// The shortest and longest idle sleep.
pub const MIN_IDLE: Duration = Duration::from_secs(1);
pub const MAX_IDLE: Duration = Duration::from_secs(60);

/// The wait after a wake that failed, given the one before it: twice `MIN_IDLE` at first, then
/// doubled each time, at most `MAX_IDLE`.
pub(super) fn next_wake_delay(previous: Option<Duration>) -> Duration {
    previous.map_or(MIN_IDLE * 2, |d| d * 2).min(MAX_IDLE)
}

/// After a failed wake: when the worker tries again, and the wait that led there.
#[derive(Debug, Clone, Copy)]
struct WakeRetry {
    at: i64,
    delay: Duration,
}

pub struct WorkerParts {
    pub name: String,
    pub address: String,
    pub account_id: i64,
    pub provider: Arc<dyn Provider>,
    pub writer: WriterHandle,
    /// A `Store::open_reader` connection of its own.
    pub reader: Store,
    pub board: StatusBoard,
    pub signal: Arc<Notify>,
    pub clock: Clock,
    /// The name put before `address` in `From:`.
    pub from_name: Option<String>,
    /// The account's sync engine wake: nudged when a sent message lands.
    pub sync_wake: Arc<Notify>,
    /// `Paths::draft_files_dir()`: where a sent draft's imported attachments are removed from.
    pub draft_files_dir: PathBuf,
    /// The account's kind: its provider's name in a failure.
    pub kind: AccountKind,
}

pub struct Worker {
    name: String,
    address: String,
    account_id: i64,
    provider: Arc<dyn Provider>,
    writer: WriterHandle,
    reader: Arc<Mutex<Store>>,
    board: StatusBoard,
    signal: Arc<Notify>,
    clock: Clock,
    /// How often a paused worker re-reads the account's `needs_auth` flag.
    paused_poll: Duration,
    from_name: Option<String>,
    sync_wake: Arc<Notify>,
    draft_files_dir: PathBuf,
    kind: AccountKind,
    /// Rows found in flight at start, until they settle (`sending`).
    recovered: Mutex<HashSet<OutboxId>>,
    /// True while a claimed `Send` or `PushDraft` row is being carried out: a stop lets it
    /// finish.
    sending: AtomicBool,
    /// Set while wakes fail: the next is not tried before its time (`next_wake_delay`).
    wake_retry: Mutex<Option<WakeRetry>>,
}

/// What a row's provider calls left to record.
pub(super) enum Landed {
    /// Done: the row completes.
    Done,
    /// A push landed: the draft keeps the provider's ids, and the sizes it lists for its files.
    Pushed {
        draft: DraftId,
        ids: DraftIds,
        listed: Vec<ListedFile>,
    },
}

/// What the loop does next.
pub(super) enum Next {
    Again,
    Idle(Duration),
}

/// Set the account's `outbox` depth on the board from `store`.
pub fn publish_depth(board: &StatusBoard, address: &str, store: &Store, account_id: i64) {
    match store.outbox_depth(account_id) {
        Ok(counts) => board.update(address, |s| {
            s.outbox = OutboxDepth {
                pending: counts.pending,
                failed: counts.failed,
                sending: counts.sending,
                saving: counts.saving,
            }
        }),
        Err(error) => {
            tracing::warn!(account = account_id, error = %error, "could not read the outbox depth")
        }
    }
}

impl Worker {
    pub fn new(parts: WorkerParts) -> Self {
        Self {
            name: parts.name,
            address: parts.address,
            account_id: parts.account_id,
            provider: parts.provider,
            writer: parts.writer,
            reader: Arc::new(Mutex::new(parts.reader)),
            board: parts.board,
            signal: parts.signal,
            clock: parts.clock,
            paused_poll: MAX_IDLE,
            from_name: parts.from_name,
            sync_wake: parts.sync_wake,
            draft_files_dir: parts.draft_files_dir,
            kind: parts.kind,
            recovered: Mutex::new(HashSet::new()),
            sending: AtomicBool::new(false),
            wake_retry: Mutex::new(None),
        }
    }

    /// Recover inflight rows, then send until `stop` turns true or its sender goes.
    pub async fn run(self, mut stop: watch::Receiver<bool>) {
        self.recover().await;
        self.publish().await;
        loop {
            if self.paused() {
                // The store can still put snoozed threads back; their calls wait for the login.
                self.wake_due().await;
                tokio::select! {
                    () = self.signal.notified() => continue,
                    // The engine clears `needs_auth` without telling the worker: look again.
                    () = tokio::time::sleep(self.paused_poll) => continue,
                    () = stopped(&mut stop) => break,
                }
            }
            let Some(next) = self.step_or_stop(&mut stop).await else {
                break;
            };
            if let Next::Idle(wait) = next {
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = self.signal.notified() => {}
                    () = stopped(&mut stop) => break,
                }
            }
        }
    }

    /// One step, or `None` once `stop` comes first; a draft send in flight is let finish
    /// (and recorded) first, within the manager's stop deadline.
    async fn step_or_stop(&self, stop: &mut watch::Receiver<bool>) -> Option<Next> {
        let mut step = pin!(self.step());
        tokio::select! {
            next = &mut step => return Some(next),
            () = stopped(stop) => {}
        }
        if self.sending.load(Ordering::SeqCst) {
            step.await;
        }
        None
    }

    fn paused(&self) -> bool {
        self.board.get(&self.address).is_some_and(|s| s.needs_auth)
    }

    async fn read<T: Send + 'static>(
        &self,
        read: impl FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, OpError> {
        let reader = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            read(&reader.lock().unwrap_or_else(PoisonError::into_inner))
        })
        .await
        .map_err(|_| OpError::internal("an outbox read failed to run"))?
        .map_err(OpError::from)
    }

    async fn publish(&self) {
        let (board, address, id) = (self.board.clone(), self.address.clone(), self.account_id);
        let _ = self
            .read(move |store| {
                publish_depth(&board, &address, store, id);
                Ok(())
            })
            .await;
    }

    /// Claim and send one row; or how long to sleep.
    pub(super) async fn step(&self) -> Next {
        self.wake_due().await;
        let claimed = self
            .writer
            .outbox(OutboxWrite::Claim {
                account_id: self.account_id,
            })
            .await;
        let row = match claimed {
            Ok(OutboxOutcome::Claimed(Some(row))) => row,
            Ok(_) => return Next::Idle(self.idle().await),
            Err(error) => {
                tracing::warn!(account = %self.name, error = %error.message, "could not claim an outbox row");
                return Next::Idle(MAX_IDLE);
            }
        };
        if matches!(row.action, Action::Send { .. } | Action::PushDraft { .. }) {
            self.sending.store(true, Ordering::SeqCst);
            // The status shows "sending…" or "saving…" while the row is inflight.
            self.publish().await;
        }
        let result = self.send(&row).await;
        self.settle(row, result).await;
        self.sending.store(false, Ordering::SeqCst);
        self.publish().await;
        Next::Again
    }

    /// Until the earliest due row or snooze (not before a failed wake's retry), within
    /// `MIN_IDLE..=MAX_IDLE`.
    async fn idle(&self) -> Duration {
        let (id, retry) = (self.account_id, self.wake_retry_at());
        let due = self
            .read(move |store| {
                let row = store.outbox_next_due(id)?;
                let snooze = store.snooze_next_due(id)?.map(|at| at.max(retry));
                Ok(row.into_iter().chain(snooze).min())
            })
            .await
            .ok()
            .flatten();
        let now = (self.clock)();
        due.map_or(MAX_IDLE, |at| {
            Duration::from_secs(u64::try_from(at - now).unwrap_or(0))
        })
        .clamp(MIN_IDLE, MAX_IDLE)
    }

    /// Put back the account's snoozes due by the worker's clock: the store queues their return
    /// (sent as any row is, once the account can send) and the status board tells who watches.
    async fn wake_due(&self) {
        let (id, now) = (self.account_id, (self.clock)());
        if now < self.wake_retry_at() {
            return;
        }
        let next = match self.read(move |store| store.snooze_next_due(id)).await {
            Ok(next) => next,
            Err(error) => {
                tracing::warn!(account = %self.name, error = %error.message, "could not read when a snooze is due");
                return;
            }
        };
        if next.is_none_or(|at| at > now) {
            // Nothing due: a wait left by an earlier failure starts afresh next time.
            self.wake_tried(now, true);
            return;
        }
        let wake = OutboxWrite::Wake { account_id: id };
        let outcome = self.writer.outbox_at(wake, now).await;
        self.wake_tried(now, outcome.is_ok());
        match outcome {
            Ok(OutboxOutcome::Woke { threads }) if threads.is_empty() => {}
            Ok(OutboxOutcome::Woke { threads }) => {
                tracing::info!(account = %self.name, threads = threads.len(), "snoozed threads went back");
                self.publish().await;
                let account = self.name.clone();
                self.board.report_woke(&SnoozeWoke { account, threads });
            }
            Ok(_) => tracing::warn!(account = %self.name, "the store answered a wake unexpectedly"),
            Err(error) => {
                tracing::warn!(account = %self.name, error = %error.message, "could not put snoozed threads back");
            }
        }
    }

    /// When a failed wake may be tried again; `i64::MIN` while wakes succeed.
    fn wake_retry_at(&self) -> i64 {
        let retry = self
            .wake_retry
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        retry.map_or(i64::MIN, |r| r.at)
    }

    /// A wake at `now` succeeded (the wait resets) or failed (the wait grows).
    fn wake_tried(&self, now: i64, ok: bool) {
        let mut retry = self
            .wake_retry
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *retry = (!ok).then(|| {
            let delay = next_wake_delay(retry.map(|r| r.delay));
            let secs = i64::try_from(delay.as_secs()).unwrap_or(i64::MAX);
            WakeRetry {
                at: now.saturating_add(secs),
                delay,
            }
        });
    }

    async fn send(&self, row: &OutboxRow) -> Result<Landed, ProviderError> {
        let target = row.clone();
        let planned = self.read(move |store| plan::plan(store, &target)).await;
        let planned = planned.map_err(|e| ProviderError::transient(&e.message))?;
        let done = |()| Landed::Done;
        match planned {
            Plan::Nothing => Ok(Landed::Done),
            Plan::LabelGone => Err(ProviderError::permanent(&gone_words(self.kind))),
            Plan::Send { threads, change } => unobserved(self.call_each(&threads, &change))
                .await
                .map(done),
            Plan::SendDraft(draft) => unobserved(self.send_draft(row, draft)).await.map(done),
            Plan::PushDraft(draft) => unobserved(self.push_draft(draft))
                .await
                .map(|(ids, listed)| Landed::Pushed { draft, ids, listed }),
            Plan::DeleteDraft(id) => unobserved(self.delete_remote_draft(&id)).await.map(done),
        }
    }

    /// One provider call per thread, stopping at the first error: a failure part-way fails the
    /// whole row (threads the provider already changed come back through the next sync).
    async fn call_each(
        &self,
        threads: &[ThreadTarget],
        change: &Change,
    ) -> Result<(), ProviderError> {
        for thread in threads {
            self.provider
                .apply(std::slice::from_ref(thread), change)
                .await?;
        }
        Ok(())
    }

    /// The write that records `result` for `row`.
    async fn write_for(
        &self,
        row: &OutboxRow,
        result: &Result<Landed, ProviderError>,
    ) -> OutboxWrite {
        match result {
            Ok(Landed::Done) => OutboxWrite::Complete {
                id: row.id,
                draft_consumed: self.provider.send_consumes_draft(),
            },
            Ok(Landed::Pushed { draft, ids, listed }) => OutboxWrite::CompletePush {
                id: row.id,
                draft: *draft,
                remote_draft_id: ids.draft_id.clone(),
                remote_message_id: ids.message_id.clone(),
                listed: listed.clone(),
            },
            Err(ProviderError::NeedsAuth { .. }) => {
                self.needs_login().await;
                OutboxWrite::Release { id: row.id }
            }
            Err(error @ ProviderError::Transient { .. }) => {
                let wait = retry_delay(row.attempts.saturating_add(1));
                let retry_at = (self.clock)() + i64::try_from(wait.as_secs()).unwrap_or(900);
                OutboxWrite::Fail {
                    id: row.id,
                    failure: Failure::Transient {
                        error: words(error),
                        retry_at,
                    },
                }
            }
            Err(error) => OutboxWrite::Fail {
                id: row.id,
                failure: Failure::Permanent(words(error)),
            },
        }
    }

    async fn settle(&self, row: OutboxRow, result: Result<Landed, ProviderError>) {
        let write = self.write_for(&row, &result).await;
        let released = matches!(write, OutboxWrite::Release { .. });
        // A completed draft row changed Gmail's Sent or Drafts: the next sync brings it.
        let landed = result.is_ok() && row.action.draft_id().is_some();
        // The completion deletes a sent draft, and its token with it: read the files first.
        let files = match (&row.action, &result) {
            (Action::Send { draft }, Ok(_)) => self.import_dir_of(*draft).await,
            _ => None,
        };
        let done = match row.action {
            Action::PushDraft { .. } | Action::DeleteDraft { .. } => "done",
            _ => "sent",
        };
        tracing::info!(account = %self.name, row = row.id, threads = row.thread_ids.len(), outcome = result.as_ref().err().map_or(done, ProviderError::code), "outbox row");
        let outcome = self.commit_settlement(row.id, &write).await;
        if outcome.is_ok() && !released {
            self.forget_recovered(row.id);
        }
        match outcome {
            Ok(OutboxOutcome::Failed {
                gave_up: true,
                row: Some(failed),
            }) => {
                // A client reading `sync.status` on `action.failed` must already see the row.
                self.publish().await;
                self.notify(failed, result.err());
            }
            Ok(OutboxOutcome::Done) if landed => {
                self.sync_wake.notify_one();
                if let (Some(dir), Some(draft)) = (files, row.action.draft_id()) {
                    remove_import_dir(draft, dir).await;
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(account = %self.name, row = row.id, error = %error.message, "could not record an outbox result")
            }
        }
    }

    /// Keep the provider result until the writer records it. Retry only this transaction:
    /// sending or creating a draft again could duplicate it. Even a storage error classified
    /// permanent (disk full, for example) can be repaired while this writer is alive.
    async fn commit_settlement(
        &self,
        row: OutboxId,
        write: &OutboxWrite,
    ) -> Result<OutboxOutcome, OpError> {
        let mut wait = MIN_IDLE;
        loop {
            match self.writer.outbox(write.clone()).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) if self.writer.is_closed() => return Err(error),
                Err(error) => {
                    tracing::warn!(account = %self.name, row, error = %error.message,
                        retry_secs = wait.as_secs(), "could not record an outbox result; retrying the write");
                    tokio::time::sleep(wait).await;
                    wait = wait.saturating_mul(2).min(MAX_IDLE);
                }
            }
        }
    }

    /// The account needs a login: the status and the store say so, as a failed pass would.
    async fn needs_login(&self) {
        self.board.update(&self.address, |s| {
            s.needs_auth = true;
            s.phase = SyncPhase::NeedsAuth;
        });
        let state = StateUpdate {
            needs_auth: Some(true),
            ..StateUpdate::default()
        };
        if let Err(error) = self
            .writer
            .write(Batch {
                state,
                ..Batch::new(self.account_id)
            })
            .await
        {
            tracing::warn!(account = %self.name, error = %error.message, "could not record needs_auth");
        }
    }

    fn notify(&self, row: OutboxRow, cause: Option<ProviderError>) {
        let mut error = match cause {
            Some(cause) => OpError::from(cause),
            None => OpError::new(
                ErrorCode::Provider,
                ErrorClass::Permanent,
                format!("{} refused the change", self.kind.provider_name()),
            ),
        };
        if let Some(last) = row
            .last_error
            .as_deref()
            .filter(|e| e.starts_with("gave up"))
        {
            // A row that gave up is final, whatever its last error was.
            error.message = last.split(':').next().unwrap_or(last).to_string();
            error.code = ErrorCode::Provider;
            error.class = ErrorClass::Permanent;
        }
        self.board.report_failure(&ActionFailed {
            account: self.name.clone(),
            row: row.id,
            threads: row.thread_ids,
            draft: row.action.draft_id(),
            action: row.action,
            error,
            client_token: row.client_token,
        });
    }
}

/// Rail's words for a failure, with the provider's after a colon (kept in `last_error`).
fn words(error: &ProviderError) -> String {
    match error.detail() {
        Some(detail) => format!("{error}: {detail}"),
        None => error.to_string(),
    }
}

/// Rail's words when a folder a row names is gone from the store: a label for Gmail, the
/// move's folder for a one-folder mailbox.
fn gone_words(kind: AccountKind) -> String {
    let provider = kind.provider_name();
    match kind.folder_model() {
        FolderModel::Labels => format!("a label of this action no longer exists in {provider}"),
        FolderModel::SingleFolder => {
            format!("the folder for this move no longer exists in {provider}")
        }
    }
}
