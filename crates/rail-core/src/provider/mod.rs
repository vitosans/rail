//! Mail providers: the read side of the `Provider` trait, the types it speaks, the HTTP layer
//! providers share, and the Gmail and Microsoft Graph implementations.

pub mod auth_results;
#[cfg(test)]
mod auth_results_tests;
pub mod error;
pub mod gmail;
pub mod graph;
pub mod html;
#[cfg(test)]
mod html_image_tests;
#[cfg(test)]
mod html_style_tests;
#[cfg(test)]
mod html_tests;
pub mod http;
pub(crate) mod limits;
pub mod links;
#[cfg(test)]
mod links_tests;
pub mod tokens;
pub mod types;

#[cfg(test)]
pub(crate) mod mock;

pub use error::ProviderError;
pub use types::{
    AttachmentMeta, Body, Carried, Change, ChangeSet, DraftIds, Envelope, EnvelopePage, FolderKind,
    INBOX_LABEL, IdPage, MAX_PAGE_TOKEN_BYTES, Mailbox, Outgoing, OutgoingAttachment, OutgoingMail,
    PageToken, Profile, Relabel, RemoteFolder, RemoteId, STARRED_LABEL, SavedDraft, SentReceipt,
    SyncCursor, TargetMessage, ThreadTarget, UNREAD_LABEL, Window,
};

/// One account's mailbox, read through its provider's API. Every method speaks Rail types;
/// object-safe so the daemon can keep one `Arc<dyn Provider>` per account.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Every folder (Gmail: label) of the mailbox.
    async fn folders(&self) -> Result<Vec<RemoteFolder>, ProviderError>;
    /// The mailbox's current sync cursor and size, read before a backfill.
    async fn profile(&self) -> Result<Profile, ProviderError>;
    /// One page of the message ids inside `window`, in the provider's order (Gmail: roughly
    /// newest first, not strictly). `page` must come from a listing of the same `window`;
    /// `ProviderError::PageTokenRejected` means the provider refused it.
    async fn list(
        &self,
        window: &Window,
        page: Option<&PageToken>,
    ) -> Result<IdPage, ProviderError>;
    /// How many ids one `list` page holds at most.
    fn list_page_size(&self) -> usize;
    /// True when the provider's first sync is its delta (Graph): `list` finds nothing, the
    /// full backfill's sweep waits until that delta has caught up, and a grown window starts
    /// the delta over.
    fn backfills_by_delta(&self) -> bool {
        false
    }
    /// A permanent condition the provider found while passes otherwise go on (Graph: a folder
    /// it gave up syncing). The engine records a pass that went through as failed with it, so
    /// the status and `:errors` show it. `None` by default.
    fn standing_error(&self) -> Option<ProviderError> {
        None
    }
    /// False when `cursor`, the one a load caught up on, holds folders the load never walked
    /// (Graph: a folder it gave up syncing, or one of a subtree its listing skipped): the load
    /// then ends without its sweep, which would delete their mail. True by default.
    fn walked_everything(&self, _cursor: &SyncCursor) -> bool {
        true
    }
    /// What changed since `cursor`. `ProviderError::CursorExpired` means a full resync.
    async fn changes(&self, cursor: &SyncCursor) -> Result<ChangeSet, ProviderError>;
    /// One message's body, rendered to text.
    async fn body(&self, id: &str) -> Result<Body, ProviderError>;
    /// The provider's own search over the whole mailbox: matching message ids.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<RemoteId>, ProviderError>;
    /// Envelopes for these ids; ids that no longer exist are left out.
    async fn envelopes(&self, ids: &[RemoteId]) -> Result<EnvelopePage, ProviderError>;
    /// Make `change` on each thread of `threads` (the worker sends one at a time). Idempotent.
    /// Gmail acts on the thread; a one-folder provider (Graph) acts on the stored messages the
    /// scope rule (`actions::scope`) picks from each target's facts. Trash is restorable.
    async fn apply(&self, threads: &[ThreadTarget], change: &Change) -> Result<(), ProviderError>;

    /// Send `mail`. NEVER retried blindly: a request that may have reached the provider is not
    /// repeated, so a transient error here means "unknown" and the caller asks `sent_already`
    /// before it sends again. A thread or source the provider no longer has (404) sends it as
    /// a message of its own. Gmail sends the message Rail built; Graph sends its own draft of
    /// it, saving it first.
    async fn send(&self, mail: &OutgoingMail) -> Result<SentReceipt, ProviderError>;
    /// Save `mail` as the provider's draft: create it when `mail.remote` is `None`, else
    /// update that draft (one deleted at the provider is created again; one in a thread or on
    /// a source the provider no longer has is saved on its own). Its ids at the provider, and
    /// the size it lists for each attachment. A create is tried once: a transient error means
    /// unknown.
    async fn draft_save(&self, mail: &OutgoingMail) -> Result<SavedDraft, ProviderError>;
    /// Whether the provider already holds `mail` as sent, by its `Message-ID`: the check
    /// before sending a message that may have reached the provider before.
    async fn sent_already(&self, mail: &OutgoingMail) -> Result<bool, ProviderError>;
    /// True when a send consumes the provider's draft of the message (Graph: the sent message
    /// keeps the draft's id), so nothing may delete "the draft" afterwards.
    /// The worker saves such a draft and keeps its id before every send.
    fn send_consumes_draft(&self) -> bool;
    /// True when the provider's forward already carries the original's attachments (Graph's
    /// `createForward`): the caller passes them as `Carried` instead of reading them.
    fn forward_carries_originals(&self) -> bool;
    /// The bytes of one attachment part of a message.
    async fn attachment(&self, message: &RemoteId, part: &str) -> Result<Vec<u8>, ProviderError>;
    /// Delete the provider draft. `ProviderError::NotFound` when it is gone already.
    async fn draft_delete(&self, draft_id: &str) -> Result<(), ProviderError>;
    /// The provider draft whose message is `message_id`, looked for in at most
    /// `gmail::MAX_DRAFT_LIST_PAGES` pages of the draft listing.
    async fn draft_find(&self, message_id: &str) -> Result<Option<RemoteId>, ProviderError>;
}
