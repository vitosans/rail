//! The operation layer: every thing a user can do, as a name plus a request type and a response
//! type. The daemon's wire format, the MCP tool schemas and the front ends' calls all come from
//! these definitions. Operations execute only in the daemon.

pub mod accounts;
pub mod actions;
pub mod attachment_content;
pub mod attachment_image;
pub mod attachments;
pub mod body;
pub mod confirm;
pub mod daemon;
pub mod drafts;
pub mod error;
pub mod guide;
pub mod op;
pub mod pending;
pub mod presets;
pub mod remote_image;
pub mod saved;
pub mod secrets;
pub mod snooze;
pub mod status;
pub mod sync;
pub mod untrusted;

#[cfg(test)]
mod attachment_content_tests;
#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod drafts_tests;
#[cfg(test)]
mod snooze_tests;
#[cfg(test)]
mod tests;

pub use accounts::{
    AccountAddRequest, AccountPasswordCheckRequest, AccountPasswordCheckResponse,
    AccountPasswordDeleteRequest, AccountPasswordDeleteResponse, AccountPasswordSetRequest,
    AccountPasswordSetResponse, AccountRemoveRequest, AccountRemoveResponse,
    AccountValidateRequest, AccountValidateResponse, AgentAccountAdd, AgentAccountRemove,
    BeginResponse, ClientSecretInput, ConsentBeginRequest, LoginBeginRequest,
    NO_PASSWORDS_FROM_AGENTS, NO_SECRETS_FROM_AGENTS, OP_REFERENCE_PREFIX, SavedAccount,
};
pub use actions::{
    Action, ActionApplyRequest, ActionApplyResponse, ActionFailed, ActionRetryRequest,
    ActionRetryResponse, ActionUndoRequest, ActionUndoResponse, DraftId, M365_LABELS,
    M365_NO_ARCHIVE, MAX_ACTION_LABELS, MAX_ACTION_THREADS, MAX_CLIENT_TOKEN_CHARS, MAX_UNDO_ROWS,
    OutboxDepth, OutboxId, RowToken, UNDO_GRACE, checked_token, client_token,
};
pub use attachment_content::{Signature, content_matches};
pub use attachment_image::{
    AttachmentImageRequest, AttachmentImageResponse, IMAGE_STALE_AFTER, IMAGE_TYPES,
    MAX_IMAGE_BYTES, MAX_IMAGE_CACHE_BYTES, image_extension,
};
pub use attachments::{
    AttachmentOpenRequest, AttachmentOpenResponse, AttachmentOutcome, AttachmentResult,
    AttachmentSaveRequest, AttachmentSaveResponse, CONTENT_MISMATCH, MAX_DIR_BYTES,
    MAX_PART_ID_BYTES, MAX_SAVE_PARTS, OPENABLE, STALE_AFTER, not_absolute_dir, openable,
    unwritable_dir,
};
pub use body::{
    BodyFetchRequest, BodyFetchResponse, BodyFetchResult, BodyOutcome, MAX_BODY_FETCH,
    MAX_MESSAGE_ID_CHARS,
};
pub use confirm::{ConfirmPlan, MAX_NAME_CHARS, plan_digest, shown};
pub use daemon::{
    BrowserEnv, DaemonInfo, DaemonInfoRequest, DaemonStopRequest, DaemonStopResponse, HelloRequest,
    HelloResponse, MAX_BROWSER_ENV_CHARS,
};
pub use drafts::{
    AttachmentSummary, DraftAttachment, DraftDiscardRequest, DraftDiscardResponse,
    DraftImportRequest, DraftImportResponse, DraftPushRequest, DraftPushResponse, DraftSaveRequest,
    DraftSaveResponse, DraftSendRequest, DraftSendResponse, MAX_ADDRESS_CHARS, MAX_ATTACHMENTS,
    MAX_BODY_BYTES, MAX_MESSAGE_BYTES, MAX_RECIPIENTS, MAX_SUBJECT_BYTES, ReplyTo,
    estimated_raw_size, new_message_id, parse_recipient, too_large_text,
};
pub(crate) use error::write_config_text;
pub use error::{ErrorCode, OpError, preset_file_message};
pub use guide::{Audience, GuideInputs, GuideRequest, GuideResponse, GuideStep, StepKind};
pub use op::{Operation, names};
pub use pending::{
    CancelRequest, MAX_WAIT_SECS, PendingId, PendingKind, PendingOutcome, PendingState,
    WaitRequest, WaitResponse,
};
pub use presets::{
    PresetFetchRequest, PresetFetchResponse, PresetListing, PresetSummary, PresetsListRequest,
    PresetsListResponse,
};
pub use remote_image::{MAX_REMOTE_URL_BYTES, RemoteImageRequest, RemoteImageResponse};
pub use saved::{SavedSearchListResponse, SavedSearchPutRequest, SavedSearchRemoveRequest};
pub use secrets::{SecretsConfigureRequest, SecretsConfigureResponse};
pub use snooze::{
    MAX_SNOOZE_SECS, MIN_SNOOZE_SECS, NOT_IN_INBOX, SnoozeClearRequest, SnoozeClearResponse,
    SnoozeSetRequest, SnoozeSetResponse, SnoozeWoke,
};
pub use status::{
    AccountStatus, AccountSummary, AccountsListRequest, AccountsListResponse, DoctorReport,
    StatusRequest, StatusResponse, TokenState,
};
pub use sync::{
    AccountSyncStatus, AgentSyncNow, DEFAULT_SEARCH_LIMIT, HitSource, MAX_QUERY_CHARS,
    MAX_SEARCH_LIMIT, SearchFailure, SearchHit, SearchRequest, SearchResponse, SyncErrorInfo,
    SyncNowRequest, SyncNowResponse, SyncPhase, SyncStatusRequest, SyncStatusResponse, SyncTarget,
};
pub use untrusted::{Secret, Untrusted};

/// serde default for flags that are on unless the caller says otherwise.
pub(crate) fn yes() -> bool {
    true
}
