//! Operation names and the `Operation` trait that ties a name to its request and response.

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::accounts::{
    AccountAddRequest, AccountRemoveRequest, AccountRemoveResponse, BeginResponse,
    ConsentBeginRequest, LoginBeginRequest,
};
use super::actions::{
    ActionApplyRequest, ActionApplyResponse, ActionRetryRequest, ActionRetryResponse,
    ActionUndoRequest, ActionUndoResponse,
};
use super::attachment_image::{AttachmentImageRequest, AttachmentImageResponse};
use super::attachments::{
    AttachmentOpenRequest, AttachmentOpenResponse, AttachmentSaveRequest, AttachmentSaveResponse,
};
use super::body::{BodyFetchRequest, BodyFetchResponse};
use super::daemon::{
    DaemonInfo, DaemonInfoRequest, DaemonStopRequest, DaemonStopResponse, HelloRequest,
    HelloResponse,
};
use super::drafts::{
    DraftDiscardRequest, DraftDiscardResponse, DraftImportRequest, DraftImportResponse,
    DraftPushRequest, DraftPushResponse, DraftSaveRequest, DraftSaveResponse, DraftSendRequest,
    DraftSendResponse,
};
use super::guide::{GuideRequest, GuideResponse};
use super::pending::{CancelRequest, WaitRequest, WaitResponse};
use super::presets::{
    PresetFetchRequest, PresetFetchResponse, PresetsListRequest, PresetsListResponse,
};
use super::remote_image::{RemoteImageRequest, RemoteImageResponse};
use super::saved::{SavedSearchListResponse, SavedSearchPutRequest, SavedSearchRemoveRequest};
use super::secrets::{SecretsConfigureRequest, SecretsConfigureResponse};
use super::snooze::{SnoozeClearRequest, SnoozeClearResponse, SnoozeSetRequest, SnoozeSetResponse};
use super::status::{AccountsListRequest, AccountsListResponse, StatusRequest, StatusResponse};
use super::sync::{
    SearchRequest, SearchResponse, SyncNowRequest, SyncNowResponse, SyncStatusRequest,
    SyncStatusResponse,
};

/// The wire names. They never change once released.
pub mod names {
    pub const STATUS: &str = "status";
    pub const ACCOUNTS_LIST: &str = "accounts.list";
    pub const PRESETS_LIST: &str = "presets.list";
    pub const PRESET_FETCH: &str = "preset.fetch";
    pub const ACCOUNT_ADD_BEGIN: &str = "account.add.begin";
    pub const ACCOUNT_LOGIN_BEGIN: &str = "account.login.begin";
    pub const CONSENT_BEGIN: &str = "consent.begin";
    pub const ACCOUNT_REMOVE: &str = "account.remove";
    pub const SECRETS_CONFIGURE: &str = "secrets.configure";
    pub const GUIDE_PROVIDER: &str = "guide.provider";
    pub const PENDING_WAIT: &str = "pending.wait";
    pub const PENDING_CANCEL: &str = "pending.cancel";
    pub const SYNC_STATUS: &str = "sync.status";
    pub const SYNC_NOW: &str = "sync.now";
    pub const SEARCH: &str = "search";
    pub const BODY_FETCH: &str = "body.fetch";
    pub const ACTION_APPLY: &str = "action.apply";
    pub const ACTION_UNDO: &str = "action.undo";
    pub const ACTION_RETRY: &str = "action.retry";
    pub const DRAFT_SAVE: &str = "draft.save";
    pub const DRAFT_SEND: &str = "draft.send";
    pub const DRAFT_DISCARD: &str = "draft.discard";
    pub const DRAFT_PUSH: &str = "draft.push";
    pub const DRAFT_IMPORT: &str = "draft.import";
    pub const SNOOZE_SET: &str = "snooze.set";
    pub const SNOOZE_CLEAR: &str = "snooze.clear";
    pub const ATTACHMENT_SAVE: &str = "attachment.save";
    pub const ATTACHMENT_OPEN: &str = "attachment.open";
    pub const SAVED_SEARCH_PUT: &str = "saved_search.put";
    pub const SAVED_SEARCH_REMOVE: &str = "saved_search.remove";
    pub const ATTACHMENT_IMAGE: &str = "attachment.image";
    pub const IMAGE_REMOTE: &str = "image.remote";

    /// Every user operation, in the order of the specs' tables.
    pub const ALL: [&str; 32] = [
        STATUS,
        ACCOUNTS_LIST,
        PRESETS_LIST,
        PRESET_FETCH,
        ACCOUNT_ADD_BEGIN,
        ACCOUNT_LOGIN_BEGIN,
        CONSENT_BEGIN,
        ACCOUNT_REMOVE,
        SECRETS_CONFIGURE,
        GUIDE_PROVIDER,
        PENDING_WAIT,
        PENDING_CANCEL,
        SYNC_STATUS,
        SYNC_NOW,
        SEARCH,
        BODY_FETCH,
        ACTION_APPLY,
        ACTION_UNDO,
        ACTION_RETRY,
        DRAFT_SAVE,
        DRAFT_SEND,
        DRAFT_DISCARD,
        DRAFT_PUSH,
        DRAFT_IMPORT,
        SNOOZE_SET,
        SNOOZE_CLEAR,
        ATTACHMENT_SAVE,
        ATTACHMENT_OPEN,
        SAVED_SEARCH_PUT,
        SAVED_SEARCH_REMOVE,
        ATTACHMENT_IMAGE,
        IMAGE_REMOTE,
    ];

    /// The daemon's own methods: the handshake and its lifecycle. Not user operations.
    pub const HELLO: &str = "hello";
    pub const DAEMON_INFO: &str = "daemon.info";
    pub const DAEMON_STOP: &str = "daemon.stop";

    /// The notification the daemon sends to the connection that began a pending operation.
    pub const PENDING_FINISHED: &str = "pending.finished";

    /// Sent, to connections that asked with `sync.now { watch: true }`, on every status change.
    /// It may also be sent with the status unchanged when the store changed under an open
    /// reader (an inline picture's body refresh wrote a body), so a front end reads it again.
    pub const SYNC_PROGRESS: &str = "sync.progress";

    /// Sent, on the same watch as `sync.progress`, when Gmail refused an action for good.
    pub const ACTION_FAILED: &str = "action.failed";
    /// Sent, on the same watch as `sync.progress`, when snoozed threads came back to the inbox.
    pub const SNOOZE_WOKE: &str = "snooze.woke";
}

/// One operation: its wire name and the types it takes and returns.
pub trait Operation {
    const NAME: &'static str;
    type Request: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static;
    type Response: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static;
}

macro_rules! operation {
    ($(#[$doc:meta])* $marker:ident, $name:expr, $request:ty, $response:ty) => {
        $(#[$doc])*
        pub struct $marker;

        impl Operation for $marker {
            const NAME: &'static str = $name;
            type Request = $request;
            type Response = $response;
        }
    };
}

operation!(
    /// Everything `rail doctor` reports, plus the daemon's own state.
    Status,
    names::STATUS,
    StatusRequest,
    StatusResponse
);
operation!(
    /// Configured accounts, token presence and `ai_send` policy.
    AccountsList,
    names::ACCOUNTS_LIST,
    AccountsListRequest,
    AccountsListResponse
);
operation!(
    /// Presets in the known directories.
    PresetsList,
    names::PRESETS_LIST,
    PresetsListRequest,
    PresetsListResponse
);
operation!(
    /// One preset from an `https` URL or a file.
    PresetFetch,
    names::PRESET_FETCH,
    PresetFetchRequest,
    PresetFetchResponse
);
operation!(
    /// Validate a new account, start its login and return the URL; persists on completion.
    AccountAddBegin,
    names::ACCOUNT_ADD_BEGIN,
    AccountAddRequest,
    BeginResponse
);
operation!(
    /// Log in again to an account already in `config.toml`.
    AccountLoginBegin,
    names::ACCOUNT_LOGIN_BEGIN,
    LoginBeginRequest,
    BeginResponse
);
operation!(
    /// Microsoft admin consent for one concrete tenant.
    ConsentBegin,
    names::CONSENT_BEGIN,
    ConsentBeginRequest,
    BeginResponse
);
operation!(
    /// Remove an account's config entry and tokens; its mail rows stay.
    AccountRemove,
    names::ACCOUNT_REMOVE,
    AccountRemoveRequest,
    AccountRemoveResponse
);
operation!(
    /// Choose the token backend and optionally move existing tokens.
    SecretsConfigure,
    names::SECRETS_CONFIGURE,
    SecretsConfigureRequest,
    SecretsConfigureResponse
);
operation!(
    /// Step-by-step provider setup for an agent.
    GuideProvider,
    names::GUIDE_PROVIDER,
    GuideRequest,
    GuideResponse
);
operation!(
    /// Wait, at most five minutes per call, for a begun login or consent.
    PendingWait,
    names::PENDING_WAIT,
    WaitRequest,
    WaitResponse
);
operation!(
    /// Cancel a begun login or consent. A save in progress is never cancelled.
    PendingCancel,
    names::PENDING_CANCEL,
    CancelRequest,
    WaitResponse
);
operation!(
    /// Every account's sync status, or one account's.
    SyncStatus,
    names::SYNC_STATUS,
    SyncStatusRequest,
    SyncStatusResponse
);
operation!(
    /// Wake one account's sync, or every account's.
    SyncNow,
    names::SYNC_NOW,
    SyncNowRequest,
    SyncNowResponse
);
operation!(
    /// Search the store and the provider's whole mailbox.
    Search,
    names::SEARCH,
    SearchRequest,
    SearchResponse
);
operation!(
    /// Fetch bodies the store does not hold yet; one outcome per message, never a body.
    BodyFetch,
    names::BODY_FETCH,
    BodyFetchRequest,
    BodyFetchResponse
);
operation!(
    /// Change threads now in the store and queue the change for Gmail. Terminal-only.
    ActionApply,
    names::ACTION_APPLY,
    ActionApplyRequest,
    ActionApplyResponse
);
operation!(
    /// Take back queued changes that have not been sent. Terminal-only.
    ActionUndo,
    names::ACTION_UNDO,
    ActionUndoRequest,
    ActionUndoResponse
);
operation!(
    /// Send a refused change again. Terminal-only.
    ActionRetry,
    names::ACTION_RETRY,
    ActionRetryRequest,
    ActionRetryResponse
);
operation!(
    /// Create or update a draft, checked before anything is stored. Terminal-only.
    DraftSave,
    names::DRAFT_SAVE,
    DraftSaveRequest,
    DraftSaveResponse
);
operation!(
    /// Queue a draft to be sent once the undo grace is over. Terminal-only.
    DraftSend,
    names::DRAFT_SEND,
    DraftSendRequest,
    DraftSendResponse
);
operation!(
    /// Delete a draft that is not being sent. Terminal-only.
    DraftDiscard,
    names::DRAFT_DISCARD,
    DraftDiscardRequest,
    DraftDiscardResponse
);
operation!(
    /// Queue a draft's save to Gmail's Drafts, after the undo grace. Terminal-only.
    DraftPush,
    names::DRAFT_PUSH,
    DraftPushRequest,
    DraftPushResponse
);
operation!(
    /// Make a local draft of a Gmail draft, keeping its Gmail ids. Terminal-only.
    DraftImport,
    names::DRAFT_IMPORT,
    DraftImportRequest,
    DraftImportResponse
);
operation!(
    /// Write a message's attachments into a directory the client resolved; answers one outcome
    /// and a path per part, never bytes. Terminal-only.
    AttachmentSave,
    names::ATTACHMENT_SAVE,
    AttachmentSaveRequest,
    AttachmentSaveResponse
);
operation!(
    /// Write one attachment into a private cache directory for the system opener; answers its
    /// path, never bytes. Terminal-only.
    AttachmentOpen,
    names::ATTACHMENT_OPEN,
    AttachmentOpenRequest,
    AttachmentOpenResponse
);
operation!(
    /// Write one inline picture into Rail's image cache for the reader; answers its path,
    /// never bytes. Terminal-only.
    AttachmentImage,
    names::ATTACHMENT_IMAGE,
    AttachmentImageRequest,
    AttachmentImageResponse
);
operation!(
    /// Write one picture a mail names by its web address into Rail's image cache for the
    /// reader; answers its path, never bytes. Terminal-only.
    ImageRemote,
    names::IMAGE_REMOTE,
    RemoteImageRequest,
    RemoteImageResponse
);
operation!(
    /// Save a search, or rename one, in `config.toml`; answers the whole new list.
    /// Terminal-only.
    SavedSearchPut,
    names::SAVED_SEARCH_PUT,
    SavedSearchPutRequest,
    SavedSearchListResponse
);
operation!(
    /// Remove a saved search; the mail it finds is not touched. Terminal-only.
    SavedSearchRemove,
    names::SAVED_SEARCH_REMOVE,
    SavedSearchRemoveRequest,
    SavedSearchListResponse
);
operation!(
    /// The handshake: protocol and Rail versions.
    Hello,
    names::HELLO,
    HelloRequest,
    HelloResponse
);
operation!(
    /// The daemon's pid, version, socket and load, without touching config or store.
    DaemonInfoOp,
    names::DAEMON_INFO,
    DaemonInfoRequest,
    DaemonInfo
);
operation!(
    /// Ask the daemon to exit once running saves finish.
    DaemonStop,
    names::DAEMON_STOP,
    DaemonStopRequest,
    DaemonStopResponse
);
operation!(
    /// Archive threads now and put them back at a time. Terminal-only.
    SnoozeSet,
    names::SNOOZE_SET,
    SnoozeSetRequest,
    SnoozeSetResponse
);
operation!(
    /// Put snoozed threads back now. Terminal-only.
    SnoozeClear,
    names::SNOOZE_CLEAR,
    SnoozeClearRequest,
    SnoozeClearResponse
);
