//! Microsoft 365 through Microsoft Graph v1.0, over the shared `ApiClient` in its Graph dialect
//! (immutable ids on every request, Graph's error envelope, at most `GRAPH_CONCURRENCY`
//! requests in flight): folders, the delta under one combined cursor, search, envelopes,
//! bodies and attachments, thread actions (moves and PATCHes per message through `$batch`), and
//! writing through Outlook drafts (create, update, attachments, upload sessions, send).
//! Graph's own JSON never leaves this module; every string it returns is outside text.

mod actions;
mod attachment;
mod batch;
mod body;
mod cursor;
mod dates;
mod delta;
mod drafts;
pub mod errors;
mod folders;
mod message;
mod reconcile;
mod search;
mod send;
mod stuck;
mod upload;
mod wire;

#[cfg(test)]
mod actions_tests;
#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod body_tests;
#[cfg(test)]
mod delta_tests;
#[cfg(test)]
mod drafts_tests;
#[cfg(test)]
mod errors_tests;
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod folders_tests;
#[cfg(test)]
mod message_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod send_tests;
#[cfg(test)]
mod unsnooze_tests;
#[cfg(test)]
mod upload_tests;

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::http::ApiClient;
use super::{
    Body, Change, ChangeSet, EnvelopePage, FolderKind, IdPage, OutgoingMail, PageToken, Profile,
    ProviderError, RemoteFolder, RemoteId, SavedDraft, SentReceipt, SyncCursor, ThreadTarget,
    Window,
};
use crate::config::SyncSettings;

pub use actions::MOVE_TARGET_GONE;
pub use attachment::REFERENCE_ATTACHMENT;
pub use batch::{MAX_BATCH, MAX_ITEM_RETRY_AFTER};
pub use body::{BODY_FIELDS, HTML_BODY};
pub use cursor::{MAX_CURSOR_BYTES, MAX_LINK_BYTES};
pub use delta::{GRAPH_PAGES_PER_CALL, PAGE_SIZE};
pub use folders::{MAX_FOLDER_DEPTH, MAX_FOLDERS, SKIPPED_ROOTS, WELL_KNOWN};
pub use reconcile::SMALL_ATTACHMENT_BYTES;
pub use search::{MAX_GRAPH_SEARCH, MESSAGE_FIELDS};
pub use stuck::MAX_LINK_RESETS;
pub use upload::{UPLOAD_CHUNK, UPLOAD_TRIES};

/// The API root every request is relative to; `me` is the signed-in mailbox.
pub const GRAPH_API: &str = "https://graph.microsoft.com/v1.0/";
/// Requests in flight at once for one mailbox: Outlook's limit per app and mailbox.
pub const GRAPH_CONCURRENCY: u32 = 4;
/// Messages per delta page Rail asks for (`Prefer: odata.maxpagesize`).
pub const DELTA_PAGE: u32 = 100;
/// The longest Graph id Rail puts in a path.
pub const MAX_GRAPH_ID_BYTES: usize = 512;
/// Quota units per request: Graph counts requests (10,000 per 10 minutes per app and mailbox).
pub(crate) const COST_REQUEST: u32 = 1;

/// Microsoft 365 through Graph.
pub struct GraphProvider {
    api: ApiClient,
    /// The account's `history_days` (0: the whole mailbox): the cutoff of a new cursor.
    history_days: u32,
    /// What the last folder listing found: the delta's folders and their order.
    folders: Mutex<Vec<RemoteFolder>>,
    /// The folders of `folders` whose parent's child listing answered not found in the last
    /// listing: kept as known, not walked this pass.
    skipped: Mutex<HashSet<RemoteId>>,
    /// Folders whose delta links keep running over the cap, for the daemon's life.
    stuck: Mutex<stuck::Stuck>,
    /// The domain of the account's own address: the only one Exchange's stamp of internal
    /// mail vouches for (`body`). `None` vouches for none.
    own_domain: Option<String>,
}

impl GraphProvider {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api,
            history_days: 0,
            folders: Mutex::new(Vec::new()),
            skipped: Mutex::new(HashSet::new()),
            stuck: Mutex::new(stuck::Stuck::default()),
            own_domain: None,
        }
    }

    /// The account's own address, whose domain internal mail may be vouched for.
    pub fn with_own_address(mut self, address: &str) -> Self {
        self.own_domain = crate::provider::auth_results::address_domain(address);
        self
    }

    /// Initial deltas ask only for mail received in the last `days` (0: all of it).
    pub fn with_history_days(mut self, days: u32) -> Self {
        self.history_days = days;
        self
    }

    fn stuck(&self) -> std::sync::MutexGuard<'_, stuck::Stuck> {
        self.stuck.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// List the folders, and remember them.
    async fn listed_folders(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let listed = self.list_folders().await?;
        *self.folders.lock().unwrap_or_else(PoisonError::into_inner) = listed.clone();
        Ok(listed)
    }

    /// The folders the last listing found; listed now when none has run.
    pub(crate) async fn known_folders(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let cached = self
            .folders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if cached.is_empty() {
            self.listed_folders().await
        } else {
            Ok(cached)
        }
    }

    /// The id of the folder of `kind` the last listing found; `None` before any listing.
    pub(crate) fn known_folder(&self, kind: FolderKind) -> Option<RemoteId> {
        let folders = self.folders.lock().unwrap_or_else(PoisonError::into_inner);
        folders
            .iter()
            .find(|f| f.kind == kind)
            .map(|f| f.remote_id.clone())
    }

    /// Whether the last listing found `folder` (true before any listing).
    pub(crate) fn syncs(&self, folder: &str) -> bool {
        let folders = self.folders.lock().unwrap_or_else(PoisonError::into_inner);
        folders.is_empty() || folders.iter().any(|f| f.remote_id == folder)
    }

    /// The receive-time cutoff of a new cursor, as the engine's window computes it.
    fn cutoff_ms(&self) -> Option<i64> {
        let settings = SyncSettings {
            history_days: self.history_days,
            body_cache_days: 0,
            poll_interval: Duration::ZERO,
        };
        settings.window_start_ms(crate::auth::unix_now())
    }
}

/// Whether `text` is a Graph id Rail puts in a URL path: 1 to `MAX_GRAPH_ID_BYTES` bytes of
/// ASCII letters, digits, `-`, `_` and `=` (Graph's URL-safe base64).
pub(crate) fn graph_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_GRAPH_ID_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_=".contains(&b))
}

#[async_trait::async_trait]
impl super::Provider for GraphProvider {
    async fn folders(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        self.listed_folders().await
    }

    async fn profile(&self) -> Result<Profile, ProviderError> {
        self.read_profile().await
    }

    /// The first sync is the delta (`profile` leaves every folder unstarted): no listing.
    async fn list(&self, _: &Window, _: Option<&PageToken>) -> Result<IdPage, ProviderError> {
        Ok(IdPage::default())
    }

    fn list_page_size(&self) -> usize {
        DELTA_PAGE as usize
    }

    fn backfills_by_delta(&self) -> bool {
        true
    }

    fn standing_error(&self) -> Option<ProviderError> {
        self.stuck().error()
    }

    /// From the cursor itself, so it holds across a restart: false while a folder in it is
    /// caught up with no link (set aside, given up on), or when it does not read.
    fn walked_everything(&self, cursor: &SyncCursor) -> bool {
        cursor::GraphCursor::decode(cursor).is_ok_and(|c| c.walked_all())
    }

    async fn changes(&self, cursor: &SyncCursor) -> Result<ChangeSet, ProviderError> {
        self.delta(cursor).await
    }

    async fn body(&self, id: &str) -> Result<Body, ProviderError> {
        self.read_body(id).await
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<RemoteId>, ProviderError> {
        self.search_ids(query, limit).await
    }

    async fn envelopes(&self, ids: &[RemoteId]) -> Result<EnvelopePage, ProviderError> {
        self.envelopes_of(ids).await
    }

    async fn apply(&self, threads: &[ThreadTarget], change: &Change) -> Result<(), ProviderError> {
        self.apply_change(threads, change).await
    }

    async fn send(&self, mail: &OutgoingMail) -> Result<SentReceipt, ProviderError> {
        self.send_mail(mail).await
    }

    async fn draft_save(&self, mail: &OutgoingMail) -> Result<SavedDraft, ProviderError> {
        self.save_draft(mail).await
    }

    async fn sent_already(&self, mail: &OutgoingMail) -> Result<bool, ProviderError> {
        self.was_sent(mail).await
    }

    /// Graph sends its draft itself: the sent message keeps the draft's (immutable) id.
    fn send_consumes_draft(&self) -> bool {
        true
    }

    /// `createForward` copies the original's attachments onto the draft.
    fn forward_carries_originals(&self) -> bool {
        true
    }

    async fn attachment(&self, message: &RemoteId, part: &str) -> Result<Vec<u8>, ProviderError> {
        self.attachment_bytes(message, part).await
    }

    async fn draft_delete(&self, draft_id: &str) -> Result<(), ProviderError> {
        self.delete_draft(draft_id).await
    }

    async fn draft_find(&self, message_id: &str) -> Result<Option<RemoteId>, ProviderError> {
        self.find_draft(message_id).await
    }
}
