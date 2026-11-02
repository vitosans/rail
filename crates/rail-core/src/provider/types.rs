//! The mail types every provider speaks. No provider's own JSON leaks past its module.

use serde::{Deserialize, Serialize};

use crate::store::drafts::DraftKind;

/// A provider's identifier for a message, thread or folder.
pub type RemoteId = String;

/// Gmail's system label ids: what its adapter sends for archive (remove `INBOX`), read (remove
/// `UNREAD`) and star (add `STARRED`).
pub const INBOX_LABEL: &str = "INBOX";
pub const UNREAD_LABEL: &str = "UNREAD";
pub const STARRED_LABEL: &str = "STARRED";

/// Where the next delta starts (Gmail: a `historyId`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncCursor(pub String);

/// The next page of a listing (Gmail's `nextPageToken`). A backfill stores it with the page it
/// follows, so a later pass goes on with the same listing; it is only ever valid for the query
/// it was issued for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageToken(pub String);

/// The longest page token Rail sends back or stores.
pub const MAX_PAGE_TOKEN_BYTES: usize = 512;

impl PageToken {
    /// `text` as a page token, if Rail accepts it: 1 to `MAX_PAGE_TOKEN_BYTES` bytes of ASCII
    /// letters, digits and `-_.~+/=`. It is outside text, sent back only as a percent-encoded
    /// query value, never in a path.
    pub fn checked(text: &str) -> Option<Self> {
        let safe = !text.is_empty()
            && text.len() <= MAX_PAGE_TOKEN_BYTES
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.~+/=".contains(&b));
        safe.then(|| Self(text.to_string()))
    }
}

/// The ids of a draft saved at the provider: the draft itself, and the message it holds (which
/// changes with every update).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftIds {
    pub draft_id: RemoteId,
    pub message_id: RemoteId,
}

/// The `folders.kind` values of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Spam,
    Archive,
    /// A provider's own label that is not one of the above (Gmail's `IMPORTANT`, categories).
    Label,
    /// A label or folder the user made.
    User,
}

impl FolderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Spam => "spam",
            Self::Archive => "archive",
            Self::Label => "label",
            Self::User => "user",
        }
    }

    /// The kind a `folders.kind` value names; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "inbox" => Self::Inbox,
            "sent" => Self::Sent,
            "drafts" => Self::Drafts,
            "trash" => Self::Trash,
            "spam" => Self::Spam,
            "archive" => Self::Archive,
            "label" => Self::Label,
            "user" => Self::User,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    pub remote_id: RemoteId,
    pub name: String,
    pub kind: FolderKind,
    /// The parent folder's `remote_id` (Graph's nested folders); `None` at the top (Gmail:
    /// always, its nesting lives in the `Parent/Child` name).
    pub parent: Option<RemoteId>,
}

/// One address of a `From`, `To`, `Cc` or `Bcc` header, as stored in `*_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    #[serde(default)]
    pub name: Option<String>,
    pub address: String,
}

/// A message's metadata. Every string is mail content: outside text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Envelope {
    pub remote_id: RemoteId,
    pub thread_id: RemoteId,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Vec<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    /// The `Reply-To:` header's mailboxes; empty when it is missing.
    pub reply_to: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    /// The `Date:` header in Unix seconds (the receive time when it is missing or absurd).
    pub date: i64,
    /// When the provider received it, in milliseconds (Gmail's `internalDate`).
    pub internal_date: i64,
    pub snippet: String,
    /// Folder (label) ids, without the flag labels.
    pub folders: Vec<RemoteId>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub size: i64,
}

/// Which receive times a listing covers, in milliseconds. Providers compare in whole seconds,
/// as Gmail's `after:` and `before:` do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    /// Received at or after this.
    pub after_ms: Option<i64>,
    /// Received before this.
    pub before_ms: Option<i64>,
}

impl Window {
    pub fn contains(&self, internal_ms: i64) -> bool {
        let second = internal_ms.div_euclid(1000);
        self.after_ms.is_none_or(|a| second >= a.div_euclid(1000))
            && self.before_ms.is_none_or(|b| second < b.div_euclid(1000))
    }
}

/// One page of a listing: message ids in the provider's order, which for Gmail is only
/// roughly newest first (the live test saw spreads of hours).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdPage {
    pub ids: Vec<RemoteId>,
    pub next: Option<PageToken>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvelopePage {
    pub envelopes: Vec<Envelope>,
    /// Ids whose metadata could not be read; logged by id and retried by the next full backfill.
    pub skipped: Vec<RemoteId>,
    pub next: Option<PageToken>,
}

/// A change to one message's folders and flags. `None` leaves a flag as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relabel {
    pub remote_id: RemoteId,
    pub add_folders: Vec<RemoteId>,
    pub remove_folders: Vec<RemoteId>,
    pub read: Option<bool>,
    pub starred: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub added: Vec<Envelope>,
    pub deleted: Vec<RemoteId>,
    pub relabeled: Vec<Relabel>,
    pub skipped: Vec<RemoteId>,
    /// Where the next call starts.
    pub cursor: SyncCursor,
    /// True when more changes wait after `cursor`: call again.
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMeta {
    pub part_id: String,
    /// Outside text.
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    /// A file Rail fetches and attaches again (a forwarded original, an imported draft's
    /// file): every Gmail part, and on Graph a `fileAttachment` that is not inline. Graph's
    /// attached messages and links stay where Outlook keeps them, and its inline pictures go
    /// when Rail writes the text body.
    pub plain_file: bool,
    /// The part's Content-ID without its angle brackets: what a `cid:` address in the HTML
    /// names. `None` when it has none, or one Rail does not keep (`content_id`).
    pub content_id: Option<String>,
}

/// The longest Content-ID Rail keeps, in bytes.
pub(crate) const MAX_CONTENT_ID_BYTES: usize = 255;

/// A Content-ID header's value (`<logo@x>`, with or without the brackets) as Rail keeps it:
/// printable ASCII without spaces, 1 to `MAX_CONTENT_ID_BYTES` bytes; anything else is `None`.
pub(crate) fn content_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let id = trimmed
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(trimmed);
    let kept = !id.is_empty()
        && id.len() <= MAX_CONTENT_ID_BYTES
        && id.bytes().all(|b| b.is_ascii_graphic());
    kept.then(|| id.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body {
    /// Rail's rendered plain text: what FTS indexes and replies quote.
    pub text: String,
    /// The HTML part as received, never rendered by a browser engine.
    pub html: Option<String>,
    /// True when the body was over `MAX_BODY_BYTES` and `text` ends with the marker.
    pub truncated: bool,
    pub attachments: Vec<AttachmentMeta>,
    /// True when the provider could not list the attachments: `attachments` is then empty and
    /// says nothing, and the ones already stored are kept.
    pub attachments_unknown: bool,
    /// The addresses the text's `[n]` markers stand for (link `n` is `links[n - 1]`), at most
    /// `links::MAX_LINKS`. Outside text: validated again before use.
    pub links: Vec<String>,
    /// The From domains the receiving server vouched for (`auth_results`), lower case: empty
    /// when it vouched for none or did not say.
    pub vouched_domains: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub cursor: SyncCursor,
    /// The mailbox's size, when the provider can tell it cheaply (Gmail can; Graph cannot).
    pub messages_total: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_token_is_checked_like_other_gmail_values() {
        assert_eq!(
            PageToken::checked("09876543210987654321"),
            Some(PageToken("09876543210987654321".into()))
        );
        assert!(PageToken::checked("aB-_.~+/=9").is_some());
        for bad in ["", "a b", "a&b", "a%20", "tok\n", "\u{e9}", "a?b", "a#b"] {
            assert_eq!(PageToken::checked(bad), None, "{bad:?}");
        }
        assert!(PageToken::checked(&"a".repeat(MAX_PAGE_TOKEN_BYTES)).is_some());
        assert!(PageToken::checked(&"a".repeat(MAX_PAGE_TOKEN_BYTES + 1)).is_none());
    }

    #[test]
    fn a_folder_kind_reads_back_from_its_store_name() {
        use FolderKind::*;
        for kind in [Inbox, Sent, Drafts, Trash, Spam, Archive, Label, User] {
            assert_eq!(FolderKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(FolderKind::parse("starred"), None);
        assert_eq!(FolderKind::parse(""), None);
    }

    #[test]
    fn an_outgoing_mails_debug_shows_no_content_and_a_carried_one_counts_its_size() {
        use crate::store::drafts::DraftKind;
        let mail = OutgoingMail {
            outgoing: Outgoing {
                from_name: Some("Dana".into()),
                from: "dana@example.com".into(),
                to: vec![Mailbox {
                    name: None,
                    address: "sam@example.com".into(),
                }],
                cc: vec![],
                bcc: vec![],
                subject: "Secret plans".into(),
                message_id: "<x@example.com>".into(),
                in_reply_to: None,
                references: vec![],
                body: "the body".into(),
                attachments: vec![OutgoingAttachment {
                    name: "plan.pdf".into(),
                    mime: "application/pdf".into(),
                    bytes: vec![],
                    carried: Some(Carried {
                        message: "m1".into(),
                        part: "p1".into(),
                        size: 7,
                    }),
                    listed: None,
                }],
            },
            kind: DraftKind::Forward,
            source: Some("m1".into()),
            remote: None,
            thread: Some("t1".into()),
        };
        let shown = format!("{mail:?}");
        for secret in ["dana", "sam", "Secret", "the body", "plan.pdf"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert_eq!(mail.outgoing.attachments[0].size(), 7);
    }
}

/// What the daemon sends: every string is user or mail content.
#[derive(Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub from_name: Option<String>,
    pub from: String,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    pub subject: String,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub body: String,
    pub attachments: Vec<OutgoingAttachment>,
}

impl std::fmt::Debug for Outgoing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outgoing")
            .field(
                "recipients",
                &(self.to.len() + self.cc.len() + self.bcc.len()),
            )
            .field("body_bytes", &self.body.len())
            .field("attachments", &self.attachments.len())
            .finish()
    }
}

/// One file of an outgoing message.
#[derive(Clone, PartialEq, Eq)]
pub struct OutgoingAttachment {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    /// Set for a forwarded original the provider's forward already holds: `bytes` is empty.
    pub carried: Option<Carried>,
    /// The size the provider listed for its copy, once Rail uploaded or imported it (Graph
    /// lists a file larger than its bytes): matched in place of `size()` when set.
    pub listed: Option<usize>,
}

impl OutgoingAttachment {
    /// Its size in bytes: the bytes read, or a carried one's listed size.
    pub fn size(&self) -> usize {
        self.carried.as_ref().map_or(self.bytes.len(), |c| c.size)
    }
}

impl std::fmt::Debug for OutgoingAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutgoingAttachment")
            .field("bytes", &self.bytes.len())
            .field("carried", &self.carried.is_some())
            .finish()
    }
}

/// What a draft save left at the provider: its ids, and the size the provider lists for each
/// of the message's attachments, in their order (`None` for one it did not say; empty for a
/// provider that lists none, as Gmail's builds the whole message on every save).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedDraft {
    pub ids: DraftIds,
    pub listed: Vec<Option<usize>>,
}

impl From<DraftIds> for SavedDraft {
    fn from(ids: DraftIds) -> Self {
        Self {
            ids,
            listed: Vec::new(),
        }
    }
}

/// A forwarded original the provider's own forward carries (Graph's `createForward`): its
/// bytes are not read unless the provider finds its copy missing. Ids and a size only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    /// The provider id of the message it comes from.
    pub message: RemoteId,
    /// Its part (Graph: attachment) id in that message.
    pub part: String,
    /// Its size in bytes, as the store lists it.
    pub size: usize,
}

/// A message going to the provider, with what a provider that writes through its own draft
/// model needs besides it. Every string but the ids is user or mail content.
#[derive(Clone, PartialEq, Eq)]
pub struct OutgoingMail {
    pub outgoing: Outgoing,
    /// New, reply, reply-all or forward: Graph makes a reply or forward on its source.
    pub kind: DraftKind,
    /// The provider id of the message replied to or forwarded (`drafts.source_remote_id`).
    pub source: Option<RemoteId>,
    /// The provider's copy of this draft, once saved there.
    pub remote: Option<DraftIds>,
    /// Gmail's `threadId` (`drafts.remote_thread_id`).
    pub thread: Option<RemoteId>,
}

impl std::fmt::Debug for OutgoingMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutgoingMail")
            .field("outgoing", &self.outgoing)
            .field("kind", &self.kind)
            .field("source", &self.source.is_some())
            .field("remote", &self.remote.is_some())
            .field("thread", &self.thread.is_some())
            .finish()
    }
}

/// What the provider answers to an accepted send. `None`s when it accepted the message but
/// its answer could not be read: still a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentReceipt {
    pub message_id: Option<RemoteId>,
    pub thread_id: Option<RemoteId>,
}

/// One change an action asks of a provider, for whole threads. Folder ids are the provider's
/// (`folders.remote_id`); Gmail's labels are folders here too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Archive,
    Trash,
    Star {
        on: bool,
    },
    Read {
        on: bool,
    },
    Labels {
        add: Vec<RemoteId>,
        remove: Vec<RemoteId>,
    },
    Move {
        to: RemoteId,
    },
    /// Put exactly these messages back in the inbox (a snooze's wake).
    Unsnooze {
        messages: Vec<RemoteId>,
    },
}

/// One stored message of a thread an action names, with what the scope rule
/// (`actions::scope`) reads. Ids and flags only: never mail content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetMessage {
    pub id: RemoteId,
    /// The kind of its folder (Gmail: of its first label; `None` with none).
    pub folder: Option<FolderKind>,
    /// Its folder's provider id: what a move compares with its target.
    pub folder_remote: Option<RemoteId>,
    pub unread: bool,
    pub starred: bool,
    /// The `Date:` in Unix seconds: the newest message has the largest.
    pub date: i64,
}

/// One thread an action names: its provider id, and the facts of the messages the store holds
/// in it. A one-folder provider (Graph) acts on the messages the scope rule picks; Gmail acts
/// on the thread and ignores them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadTarget {
    pub thread_id: RemoteId,
    pub messages: Vec<TargetMessage>,
}

impl ThreadTarget {
    /// The thread ids of `targets`, in order.
    pub fn thread_ids(targets: &[Self]) -> Vec<RemoteId> {
        targets.iter().map(|t| t.thread_id.clone()).collect()
    }
}

/// A thread with no messages (Gmail's tests, and providers that act on threads).
impl From<&str> for ThreadTarget {
    fn from(thread_id: &str) -> Self {
        Self::from(thread_id.to_string())
    }
}

impl From<String> for ThreadTarget {
    fn from(thread_id: String) -> Self {
        Self {
            thread_id,
            messages: Vec::new(),
        }
    }
}
