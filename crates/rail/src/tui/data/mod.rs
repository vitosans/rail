//! The mail screen's two data paths, both answering through the event loop as `MailEvent`s:
//! the store, read on blocking tasks (`StoreReader`), and the daemon (`DaemonLink`). Debounce
//! timers are jobs too, so nothing runs while nothing happens.

mod attachments_link;
#[cfg(test)]
mod bulk_tests;
mod daemon_link;
mod image_link;
mod rich_read;
mod saved_link;
#[cfg(test)]
mod saved_tests;
mod store_reader;
#[cfg(test)]
mod tests;

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use futures_util::FutureExt;

use crate::tui::screens::reader::RichBody;
use rail_core::config::SavedSearch;
use rail_core::ops::{
    AccountSyncStatus, ActionApplyResponse, ActionFailed, ActionRetryResponse, ActionUndoResponse,
    AttachmentOpenResponse, AttachmentSaveResponse, BodyFetchResponse, DraftId,
    DraftImportResponse, DraftPushResponse, DraftSaveResponse, DraftSendResponse, OpError,
    OutboxId, SnoozeClearResponse, SnoozeWoke,
};
use rail_core::provider::Mailbox;
use rail_core::store::AddressEntry;
use rail_core::store::drafts::{Draft, ReplySource};
use rail_core::store::drafts_folder::DraftsRow;
use rail_core::store::view::{HtmlBody, LabelChoices, ThreadDetail, ThreadKey};

pub use daemon_link::SaveCall;
pub use daemon_link::{ApplyCall, DaemonLink};
#[cfg(test)]
pub use daemon_link::{FakeCall, FakeDaemon};
pub use store_reader::ReplyTarget;
pub use store_reader::{
    ListOutcome, ListRequest, ReadError, ScopeChoice, SidebarOutcome, StoreReader,
};

use crate::tui::app::{Effect, TaskResult, TaskSink};

/// How long the store must stay quiet after a change before the list is read again.
pub const REQUERY_DEBOUNCE: Duration = Duration::from_millis(100);
/// How long typing in the filter line must pause before the list is filtered.
pub const FILTER_DEBOUNCE: Duration = Duration::from_millis(150);

/// What a debounce timer was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Requery,
    Filter,
}

/// What the daemon link reports.
pub enum DaemonEvent {
    /// Subscribed; every account's status.
    Connected(Vec<AccountSyncStatus>),
    /// One account's status changed.
    Progress(Box<AccountSyncStatus>),
    /// The connection failed or ended.
    Lost(OpError),
    /// `sync.now` (Ctrl-r) answered.
    Refreshed(Result<(), OpError>),
    /// A snooze put threads back in the inbox (`snooze.woke`).
    Woke(SnoozeWoke),
}

/// Why an `Enter` on an attachment became a save, carried with the save so its line says so.
#[derive(Clone, PartialEq, Eq)]
pub enum NotOpened {
    /// A type outside the open list: the sender's declared type, drawn escaped.
    Type(String),
    /// The daemon found the bytes are not what the declared type says (#107).
    Contents,
}

/// One answer to the mail screen.
pub enum MailEvent {
    Listed {
        seq: u64,
        result: Result<ListOutcome, ReadError>,
    },
    Sidebar {
        seq: u64,
        result: Result<SidebarOutcome, ReadError>,
    },
    Thread {
        seq: u64,
        thread_id: i64,
        result: Result<Option<ThreadDetail>, ReadError>,
    },
    Html {
        message_id: i64,
        result: Result<Option<HtmlBody>, ReadError>,
    },
    /// An inline picture fetched and decoded; `None` when it could not be had.
    Picture {
        message_id: i64,
        source: crate::tui::screens::reader::PictureSource,
        image: Option<crate::tui::screens::reader::DecodedPicture>,
    },
    /// A message's HTML part scanned for rich mode; `None` when it has none.
    RichHtml {
        message_id: i64,
        result: Result<Option<RichBody>, ReadError>,
    },
    Bodies {
        thread_id: i64,
        result: Result<BodyFetchResponse, OpError>,
    },
    /// `action.apply` (or a snooze's `snooze.set`) answered for `call`.
    /// Boxed: with the token the call is large for an event.
    Applied {
        call: Box<ApplyCall>,
        result: Result<ActionApplyResponse, OpError>,
    },
    /// `action.undo` answered for the action with this `seq`.
    Undone {
        seq: u64,
        result: Result<ActionUndoResponse, OpError>,
    },
    Retried {
        row: OutboxId,
        result: Result<ActionRetryResponse, OpError>,
    },
    /// The daemon gave up on an action and reverted it.
    ActionFailed(Box<ActionFailed>),
    Labels {
        seq: u64,
        result: Result<LabelChoices, ReadError>,
    },
    /// `config.toml` was written for `:theme` or `:accent`; the error is a config or IO
    /// error's text, never mail.
    ThemeSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:keymap`; the error is a config or IO error's text.
    KeymapSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:hints`; the error is a config or IO error's text.
    HintsSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:clock`; the error is a config or IO error's text.
    ClockSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:time`; the error is a config or IO error's text.
    TimeSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:thread-order`; the error is a config or IO error's text.
    ThreadOrderSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:dim-read`; the error is a config or IO error's text.
    DimReadSaved {
        result: Result<(), String>,
    },
    /// The refresh with this sequence number gave its last answer, or its job panicked.
    RefreshEnded {
        seq: u64,
    },
    Daemon(DaemonEvent),
    Timer {
        token: u64,
        kind: Timer,
    },
    /// `draft.save` answered for the save with this `seq`.
    DraftSaved {
        seq: u64,
        result: Result<DraftSaveResponse, OpError>,
    },
    /// `draft.send` answered for the send with this `seq`; the token it was sent with.
    DraftSent {
        seq: u64,
        draft: DraftId,
        client_token: String,
        result: Result<DraftSendResponse, OpError>,
    },
    /// `draft.push` answered for the push with this `seq`; the token it was sent with.
    DraftPushed {
        seq: u64,
        draft: DraftId,
        client_token: String,
        result: Result<DraftPushResponse, OpError>,
    },
    /// `draft.import` answered for the Gmail draft `message`.
    DraftImported {
        seq: u64,
        message: i64,
        result: Result<DraftImportResponse, OpError>,
    },
    /// `draft.discard` answered.
    DraftDiscarded {
        draft: DraftId,
        result: Result<(), OpError>,
    },
    /// The address book's matches for a recipient prefix.
    Suggestions {
        seq: u64,
        result: Result<Vec<AddressEntry>, ReadError>,
    },
    /// The message a reply or forward is built from (`None`: it is no longer stored). Boxed:
    /// it is the largest thing any event carries.
    ReplySource {
        seq: u64,
        result: Result<Option<Box<ReplySource>>, ReadError>,
    },
    /// The Drafts folder of the accounts asked for: local drafts and Gmail's, newest first.
    DraftsFolder {
        seq: u64,
        result: Result<Vec<DraftsRow>, ReadError>,
    },
    /// A draft to resume (`None`: it is no longer stored).
    Draft {
        seq: u64,
        result: Result<Option<Box<Draft>>, ReadError>,
    },
    /// The editor exited: its exit code, or why it could not run.
    EditorDone(Result<i32, String>),
    /// `Store::matching_targets` answered for the select-all with this `seq`.
    Matched {
        seq: u64,
        result: Result<Vec<(ThreadKey, i64, bool, bool)>, ReadError>,
    },
    /// `Store::newest_sender` answered for the select-by-sender with this `seq`.
    Sender {
        seq: u64,
        result: Result<Option<Mailbox>, ReadError>,
    },
    /// When the snoozes holding the threads a snooze chooser opens for end, soonest first.
    Snoozes {
        seq: u64,
        result: Result<Vec<i64>, ReadError>,
    },
    /// One `snooze.clear` call of the "Return to inbox now" with this `seq` answered: its
    /// threads, and how many went back or why not.
    Unsnoozed {
        seq: u64,
        threads: Vec<i64>,
        result: Result<SnoozeClearResponse, OpError>,
    },
    /// How many threads a snooze put back, and the subject when it was one (`None`: it is no
    /// longer stored).
    WokeRead {
        count: usize,
        subject: Result<Option<String>, ReadError>,
    },
    /// `attachment.save` answered. `account` lets the screen name the provider in a
    /// `Missing` line; `dir` is what the status line says the files landed in; `single` is
    /// true when one part was asked for (`s` or an `Enter` Rail would not open), which is what
    /// the log says about the save. `name` is the file that save asked for, when it was one,
    /// and `unopenable` why an `Enter` did not open it when that turned into a save (FR-6,
    /// #107): both travel with the answer, so two saves in flight never cross their lines.
    AttachmentSaved {
        result: Result<AttachmentSaveResponse, OpError>,
        account: String,
        dir: String,
        single: bool,
        name: Option<String>,
        unopenable: Option<NotOpened>,
    },
    /// `attachment.open` answered; `filename` names the part in a failure's line, and
    /// `message` and `part_id` say which part to save when the daemon would not open it.
    AttachmentOpened {
        result: Result<AttachmentOpenResponse, OpError>,
        account: String,
        message: String,
        part_id: String,
        filename: String,
    },
    /// `saved_search.put` or `saved_search.remove` answered: the whole new list. `renamed` is
    /// the rename **this** request asked for, old name then new, and `None` on every other
    /// write. Every answer carries the whole list, so without it the screen cannot tell a
    /// rename's answer from the next write's — and an unrelated failure in between would lose
    /// the pair the open scope and the sidebar cursor are re-keyed through (FR-17 step 1).
    SavedSearchesChanged {
        result: Result<Vec<SavedSearch>, OpError>,
        renamed: Option<(String, String)>,
    },
    /// `config.toml` was written for `:search-counts`; the error is a config or IO error's
    /// text, never a name and never a query.
    SearchCountsSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:rich-html`.
    RichHtmlSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:images`.
    ImagesSaved {
        result: Result<(), String>,
    },
    /// `config.toml` was written for `:remote-images`.
    RemoteImagesSaved {
        result: Result<(), String>,
    },
}

/// Names and numbers only: a `MailEvent` carries mail, which must never reach a log.
impl fmt::Debug for MailEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Listed { seq, result } => f
                .debug_struct("Listed")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Sidebar { seq, result } => f
                .debug_struct("Sidebar")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Thread {
                seq,
                thread_id,
                result,
            } => f
                .debug_struct("Thread")
                .field("seq", seq)
                .field("thread_id", thread_id)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Html { message_id, .. } => f
                .debug_struct("Html")
                .field("message_id", message_id)
                .finish(),
            Self::Picture {
                message_id, image, ..
            } => f
                .debug_struct("Picture")
                .field("message_id", message_id)
                .field("ok", &image.is_some())
                .finish(),
            Self::RichHtml { message_id, .. } => f
                .debug_struct("RichHtml")
                .field("message_id", message_id)
                .finish(),
            Self::Bodies { thread_id, result } => f
                .debug_struct("Bodies")
                .field("thread_id", thread_id)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Applied { call, result } => f
                .debug_struct("Applied")
                .field("seq", &call.seq)
                .field("threads", &call.threads.len())
                .field("ok", &result.is_ok())
                .finish(),
            Self::Undone { seq, result } => f
                .debug_struct("Undone")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Retried { row, result } => f
                .debug_struct("Retried")
                .field("row", row)
                .field("ok", &result.is_ok())
                .finish(),
            Self::ActionFailed(failure) => f
                .debug_struct("ActionFailed")
                .field("row", &failure.row)
                .finish(),
            Self::Labels { seq, result } => f
                .debug_struct("Labels")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::ThemeSaved { result } => f
                .debug_struct("ThemeSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::KeymapSaved { result } => f
                .debug_struct("KeymapSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::HintsSaved { result } => f
                .debug_struct("HintsSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::ClockSaved { result } => f
                .debug_struct("ClockSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::TimeSaved { result } => f
                .debug_struct("TimeSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::ThreadOrderSaved { result } => f
                .debug_struct("ThreadOrderSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::DimReadSaved { result } => f
                .debug_struct("DimReadSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::RefreshEnded { seq } => f.debug_struct("RefreshEnded").field("seq", seq).finish(),
            Self::Daemon(event) => {
                let name = match event {
                    DaemonEvent::Connected(_) => "Connected",
                    DaemonEvent::Progress(_) => "Progress",
                    DaemonEvent::Lost(_) => "Lost",
                    DaemonEvent::Refreshed(_) => "Refreshed",
                    DaemonEvent::Woke(_) => "Woke",
                };
                write!(f, "Daemon({name})")
            }
            Self::Timer { token, kind } => f
                .debug_struct("Timer")
                .field("token", token)
                .field("kind", kind)
                .finish(),
            Self::DraftSaved { seq, result } => f
                .debug_struct("DraftSaved")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::DraftSent {
                seq, draft, result, ..
            } => f
                .debug_struct("DraftSent")
                .field("seq", seq)
                .field("draft", draft)
                .field("ok", &result.is_ok())
                .finish(),
            Self::DraftPushed {
                seq, draft, result, ..
            } => f
                .debug_struct("DraftPushed")
                .field("seq", seq)
                .field("draft", draft)
                .field("ok", &result.is_ok())
                .finish(),
            Self::DraftImported {
                seq,
                message,
                result,
            } => f
                .debug_struct("DraftImported")
                .field("seq", seq)
                .field("message", message)
                .field("ok", &result.is_ok())
                .finish(),
            Self::DraftsFolder { seq, result } => f
                .debug_struct("DraftsFolder")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::DraftDiscarded { draft, result } => f
                .debug_struct("DraftDiscarded")
                .field("draft", draft)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Suggestions { seq, result } => f
                .debug_struct("Suggestions")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::ReplySource { seq, result } => f
                .debug_struct("ReplySource")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Draft { seq, result } => f
                .debug_struct("Draft")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::EditorDone(result) => f
                .debug_struct("EditorDone")
                .field("ok", &result.is_ok())
                .finish(),
            // The tuples hold thread ids, so only how many came back is named.
            Self::Matched { seq, result } => f
                .debug_struct("Matched")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .field("len", &result.as_ref().map_or(0, Vec::len))
                .finish(),
            // `Mailbox` derives `Debug` over its name and address: neither is named here.
            Self::Sender { seq, result } => f
                .debug_struct("Sender")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            Self::Snoozes { seq, result } => f
                .debug_struct("Snoozes")
                .field("seq", seq)
                .field("ok", &result.is_ok())
                .finish(),
            // Thread ids: only how many is named.
            Self::Unsnoozed {
                seq,
                threads,
                result,
            } => f
                .debug_struct("Unsnoozed")
                .field("seq", seq)
                .field("threads", &threads.len())
                .field("ok", &result.is_ok())
                .finish(),
            Self::WokeRead { count, subject } => f
                .debug_struct("WokeRead")
                .field("count", count)
                .field("ok", &subject.is_ok())
                .finish(),
            // Never `dir`, `name`, `unopenable` or `account`: a sender-chosen name or type
            // and a user's own directory must not reach a log.
            Self::AttachmentSaved { result, single, .. } => f
                .debug_struct("AttachmentSaved")
                .field("ok", &result.is_ok())
                .field("single", single)
                .field(
                    "results",
                    &result.as_ref().map_or(0, |answer| answer.results.len()),
                )
                .finish(),
            Self::AttachmentOpened { result, .. } => f
                .debug_struct("AttachmentOpened")
                .field("ok", &result.is_ok())
                .finish(),
            // Never the pair itself: a saved search's name is outside text, so only whether
            // this answer belongs to a rename is printed.
            Self::SavedSearchesChanged { result, renamed } => f
                .debug_struct("SavedSearchesChanged")
                .field("ok", &result.is_ok())
                .field("len", &result.as_ref().map_or(0, Vec::len))
                .field("renamed", &renamed.is_some())
                .finish(),
            Self::SearchCountsSaved { result } => f
                .debug_struct("SearchCountsSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::RichHtmlSaved { result } => f
                .debug_struct("RichHtmlSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::ImagesSaved { result } => f
                .debug_struct("ImagesSaved")
                .field("ok", &result.is_ok())
                .finish(),
            Self::RemoteImagesSaved { result } => f
                .debug_struct("RemoteImagesSaved")
                .field("ok", &result.is_ok())
                .finish(),
        }
    }
}

/// Send `event` to the screen. A closed channel means the event loop has ended.
pub(crate) fn send(sink: &TaskSink, event: MailEvent) {
    let _ = sink.send(TaskResult::Mail(event));
}

/// A job that waits `delay` and reports `token`. The screen acts only on the newest token.
pub fn timer(delay: Duration, token: u64, kind: Timer) -> Effect {
    Effect::Spawn(Box::new(move |sink| {
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            send(&sink, MailEvent::Timer { token, kind });
        })
    }))
}

/// A job running `work` with the sink.
pub(crate) fn job<F, Fut>(work: F) -> Effect
where
    F: FnOnce(TaskSink) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Effect::Spawn(Box::new(move |sink| Box::pin(work(sink))))
}

/// A refresh's job: `work` sends the refresh's answers, then `RefreshEnded { seq }` follows
/// whether `work` finished or panicked, so the screen's one-refresh gate always opens again.
/// A panic then goes on to the job's supervisor, which reports it as usual.
pub(crate) fn refresh_job<F, Fut>(seq: u64, work: F) -> Effect
where
    F: FnOnce(TaskSink) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    job(move |sink| async move {
        let outcome = AssertUnwindSafe(work(sink.clone())).catch_unwind().await;
        send(&sink, MailEvent::RefreshEnded { seq });
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    })
}
