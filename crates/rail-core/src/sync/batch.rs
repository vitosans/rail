//! What the writer commits in one transaction.

use crate::provider::{Body, Envelope, PageToken, Relabel, RemoteFolder, RemoteId, Window};

/// Why a message is in the store: the sync window, or a server search that found it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    Sync,
    Search,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Search => "search",
        }
    }
}

/// Where a backfill's listing stands: Gmail's token for the next page, the listing (query) it
/// belongs to, and when it was issued (Unix seconds). Stored with the page it follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeToken {
    pub token: PageToken,
    pub listing: Window,
    pub issued_at: i64,
}

/// Columns of the account's row to change. `None` leaves a column; `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateUpdate {
    pub cursor: Option<Option<String>>,
    pub backfill_before: Option<Option<i64>>,
    pub backfill_cursor: Option<Option<String>>,
    pub backfill_window: Option<Option<i64>>,
    pub backfill_started_at: Option<Option<i64>>,
    /// The token, listing and time columns, always set (or cleared) together.
    pub backfill_resume: Option<Option<ResumeToken>>,
    pub phase: Option<String>,
    pub last_sync_at: Option<i64>,
    /// The error's code and when it happened (Unix seconds).
    pub last_error: Option<Option<(String, i64)>>,
    pub needs_auth: Option<bool>,
}

/// Delete `origin = sync` mail older than the window and search hits not searched for since
/// `search_before`, keeping starred mail and threads with mail inside the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prune {
    pub window_start_ms: Option<i64>,
    pub search_before: i64,
}

/// After a full backfill: delete `origin = sync` mail inside the window that it did not see
/// again (`synced_at` before `started_at`): Gmail no longer has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sweep {
    pub started_at: i64,
    pub window_start_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub account_id: i64,
    /// The whole folder list: folders not in it are deleted (their messages stay).
    pub folders: Option<Vec<RemoteFolder>>,
    pub upserts: Vec<Envelope>,
    /// The origin of `upserts`.
    pub origin: Origin,
    pub deletes: Vec<RemoteId>,
    /// Provider ids that changed at the same mailbox position, old to new (an IMAP move).
    pub moves: Vec<(RemoteId, RemoteId)>,
    pub relabels: Vec<Relabel>,
    pub bodies: Vec<(RemoteId, Body)>,
    /// Search hits already stored: their `searched_at` becomes now.
    pub searched: Vec<RemoteId>,
    pub sweep: Option<Sweep>,
    pub prune: Option<Prune>,
    pub state: StateUpdate,
    /// When the batch happened (Unix seconds): `synced_at`, `searched_at` and
    /// `body_fetched_at` use it. The writer's clock when `None`.
    pub at: Option<i64>,
    /// Tests: fail after every step, just before commit.
    #[cfg(test)]
    pub poison: bool,
    /// Tests: make the writer thread panic on this batch.
    #[cfg(test)]
    pub panic_writer: bool,
}

impl Batch {
    pub fn new(account_id: i64) -> Self {
        Self {
            account_id,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    pub upserted: usize,
    pub deleted: usize,
    pub swept: usize,
    pub pruned: usize,
    /// Failed outbox rows the batch's server state settled (the outbox depth changed).
    pub failed_cleared: usize,
    /// Snoozes the batch's mail made due now: the account's outbox worker wakes them.
    pub snoozes_due: usize,
}
