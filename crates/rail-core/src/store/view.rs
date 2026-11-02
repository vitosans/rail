//! What the mail screen reads: thread-list pages for a scope with keyset paging both ways, one
//! thread's messages, the sidebar's counts, and one message's HTML. The TUI runs these on a
//! read-only connection (`Store::open_reader`). Every query is parameterised and bounded; every
//! string it returns is mail content, outside text, escaped by whoever shows it.

use std::collections::HashMap;
use std::fmt;

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, params, params_from_iter};

use super::filter::Filter;
use super::snoozes::HELD;
use super::{Store, StoreError};
use crate::provider::auth_results::MAX_VOUCHED;
use crate::provider::links::MAX_LINKS;
use crate::provider::{FolderKind, Mailbox};
use crate::sync::UNREADABLE_BODY;

pub use super::view_labels::{LabelChoices, LabelOption};
pub use super::view_sidebar::{AccountCounts, FolderCount, SidebarCounts, UnifiedCounts};

/// Threads one page holds.
pub const PAGE: usize = 100;
/// Messages of one thread the reader gets: the newest ones.
pub const MAX_THREAD_MESSAGES: usize = 200;
/// Characters of one body the reader gets.
pub const MAX_SHOWN_BODY_CHARS: usize = 500_000;
/// Folders the sidebar lists for each account.
pub const MAX_SIDEBAR_FOLDERS: usize = 500;

/// Which threads a list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The inboxes of these accounts (store ids) together.
    Unified { accounts: Vec<i64> },
    /// One account's inbox.
    Inbox { account_id: i64 },
    /// One folder (Gmail label) of one account.
    Folder { account_id: i64, folder_id: i64 },
    /// Threads with a starred message in any of these accounts, in any folder.
    Starred { accounts: Vec<i64> },
    /// Threads with a message in a folder of `kind` in any of these accounts (Sent, Trash),
    /// whatever the provider calls the folder.
    Kind {
        accounts: Vec<i64>,
        kind: FolderKind,
    },
    /// Threads a snooze holds in any of these accounts: waiting for its archive, or archived
    /// until its time.
    Snoozed { accounts: Vec<i64> },
    /// Every thread of these accounts, in any folder: a saved search's base scope, narrowed by
    /// `ListQuery.saved`.
    All { accounts: Vec<i64> },
}

/// The filter's `account:`, resolved by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountFilter {
    Any,
    Only(i64),
    /// `account:` named no configured account: nothing matches.
    Nobody,
}

/// One list query: the scope, the filter, and the local time zone's offset for its dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    pub scope: Scope,
    pub filter: Filter,
    /// The open saved search's filter, `ANDed` under `filter` as its own condition. Re-derived
    /// by `filter::parse` on every read and never cached.
    pub saved: Option<Filter>,
    pub account: AccountFilter,
    /// Seconds east of UTC.
    pub utc_offset: i32,
}

/// Where a thread sorts: newest first by `last_date`, then by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadKey {
    pub last_date: i64,
    pub thread_id: i64,
}

/// Which page to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAt {
    /// The newest threads.
    First,
    /// The oldest threads.
    Last,
    /// The threads just older than this key.
    Older(ThreadKey),
    /// The threads just newer than this key.
    Newer(ThreadKey),
    /// This key and the threads older than it.
    From(ThreadKey),
}

/// One row of the list.
#[derive(Clone, PartialEq, Eq)]
pub struct ThreadRow {
    pub key: ThreadKey,
    pub account_id: i64,
    pub subject: String,
    pub participants: Vec<Mailbox>,
    pub message_count: u32,
    pub unread_count: u32,
    pub starred: bool,
    pub has_attachments: bool,
}

impl fmt::Debug for ThreadRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadRow")
            .field("key", &self.key)
            .field("account_id", &self.account_id)
            .field("message_count", &self.message_count)
            .field("unread_count", &self.unread_count)
            .finish_non_exhaustive()
    }
}

/// One page, newest first, and whether more threads lie either way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadPage {
    pub rows: Vec<ThreadRow>,
    pub older: bool,
    pub newer: bool,
}

/// One thread's messages for the reader.
#[derive(Clone, PartialEq, Eq)]
pub struct ThreadDetail {
    pub thread_id: i64,
    pub account_id: i64,
    pub subject: String,
    /// Oldest first.
    pub messages: Vec<MessageView>,
    /// Older messages beyond `MAX_THREAD_MESSAGES` that were left out.
    pub older_left_out: usize,
    /// When the snooze holding the thread ends, while one does.
    pub snoozed_until: Option<i64>,
}

impl fmt::Debug for ThreadDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadDetail")
            .field("thread_id", &self.thread_id)
            .field("messages", &self.messages)
            .finish_non_exhaustive()
    }
}

/// A message's body as the store has it. `Debug` prints the state and the length, never the
/// text.
#[derive(Clone, PartialEq, Eq)]
pub enum BodyState {
    /// Not fetched yet (older than the body cache).
    NotFetched,
    /// Fetched, and Rail could not read it.
    Unreadable,
    /// The text; `cut` when it was longer than `MAX_SHOWN_BODY_CHARS`.
    Text { text: String, cut: bool },
}

impl fmt::Debug for BodyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFetched => f.write_str("NotFetched"),
            Self::Unreadable => f.write_str("Unreadable"),
            Self::Text { text, cut } => f
                .debug_struct("Text")
                .field("chars", &text.chars().count())
                .field("cut", cut)
                .finish(),
        }
    }
}

/// An attachment's listing data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentView {
    /// The provider's own id for this part, which names it to `attachment.save` and
    /// `attachment.open`.
    pub part_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

/// One message of a thread.
#[derive(Clone, PartialEq, Eq)]
pub struct MessageView {
    pub id: i64,
    pub remote_id: String,
    pub from: Vec<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub subject: String,
    /// The `Date:` header, Unix seconds.
    pub date: i64,
    /// The receive time, Unix milliseconds.
    pub internal_date: i64,
    pub is_read: bool,
    pub is_starred: bool,
    pub snippet: String,
    pub body: BodyState,
    /// What the body's `[n]` markers stand for.
    pub links: Vec<String>,
    pub attachments: Vec<AttachmentView>,
    pub has_html: bool,
    /// The From domains the receiving server vouched for (`provider::auth_results`): empty
    /// when it vouched for none, or the body was stored before Rail kept them.
    pub vouched_domains: Vec<String>,
}

impl fmt::Debug for MessageView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match self.body {
            BodyState::NotFetched => "not fetched",
            BodyState::Unreadable => "unreadable",
            BodyState::Text { .. } => "text",
        };
        f.debug_struct("MessageView")
            .field("id", &self.id)
            .field("body", &body)
            .field("links", &self.links.len())
            .field("attachments", &self.attachments.len())
            .finish_non_exhaustive()
    }
}

/// One message's HTML, for `o`.
#[derive(Clone, PartialEq, Eq)]
pub enum HtmlBody {
    NotFetched,
    /// Fetched; the message has no HTML part.
    Absent,
    Html(String),
}

/// Never prints the HTML: only its size.
impl fmt::Debug for HtmlBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFetched => f.write_str("HtmlBody::NotFetched"),
            Self::Absent => f.write_str("HtmlBody::Absent"),
            Self::Html(html) => write!(f, "HtmlBody::Html({} bytes)", html.len()),
        }
    }
}

const THREAD_COLUMNS: &str = "t.id, t.last_date, t.account_id, t.subject, t.participants,
    t.message_count, t.unread_count, t.is_starred, t.has_attachments";
pub(super) const IN_INBOX: &str = "EXISTS (SELECT 1 FROM messages mi
    JOIN message_folders mf ON mf.message_id = mi.id JOIN folders f ON f.id = mf.folder_id
    WHERE mi.thread_id = t.id AND f.kind = 'inbox')";
const IN_FOLDER: &str = "EXISTS (SELECT 1 FROM messages mi
    JOIN message_folders mf ON mf.message_id = mi.id WHERE mi.thread_id = t.id AND mf.folder_id = ?)";

/// The threads with a message in a folder of the given kind of the given accounts (parameters:
/// the kind, then the accounts as JSON): read from the folder side, `message_folders_by_folder`
/// first, not by a probe per thread over every message.
const IN_KIND: &str = "t.id IN (SELECT mi.thread_id FROM folders f
    JOIN message_folders mf ON mf.folder_id = f.id JOIN messages mi ON mi.id = mf.message_id
    WHERE f.kind = ? AND f.account_id IN (SELECT value FROM json_each(?)))";

pub(super) fn int(n: usize) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

pub(super) fn ids_json(ids: &[i64]) -> String {
    serde_json::Value::from(ids.to_vec()).to_string()
}

pub(super) fn count(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// Senders and recipients the writer stored from `Mailbox` lists; an unreadable list (only a
/// hand-edited store has one) shows no one.
pub(super) fn mailboxes(json: &str) -> Vec<Mailbox> {
    serde_json::from_str(json).unwrap_or_default()
}

fn thread_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRow> {
    let participants: String = r.get(4)?;
    Ok(ThreadRow {
        key: ThreadKey {
            thread_id: r.get(0)?,
            last_date: r.get(1)?,
        },
        account_id: r.get(2)?,
        subject: r.get(3)?,
        participants: mailboxes(&participants),
        message_count: u32::try_from(r.get::<_, i64>(5)?).unwrap_or(u32::MAX),
        unread_count: u32::try_from(r.get::<_, i64>(6)?).unwrap_or(u32::MAX),
        starred: r.get(7)?,
        has_attachments: r.get(8)?,
    })
}

/// The scope, account and filter conditions over threads `t`, with their parameters in order.
pub(super) fn where_clause(query: &ListQuery) -> (String, Vec<Value>) {
    let (scope, mut params) = scope_condition(&query.scope);
    let mut conds = vec![scope];
    if let AccountFilter::Only(id) = query.account {
        conds.push("t.account_id = ?".to_string());
        params.push(Value::Integer(id));
    }
    // The saved filter is its own condition, pushed before the live one, so the two intersect
    // instead of one replacing the other.
    if let Some((sql, values)) = query
        .saved
        .as_ref()
        .and_then(|saved| saved.message_sql(query.utc_offset))
    {
        conds.push(sql);
        params.extend(values);
    }
    if let Some((sql, values)) = query.filter.message_sql(query.utc_offset) {
        conds.push(sql);
        params.extend(values);
    }
    (conds.join(" AND "), params)
}

/// A scope's condition over threads `t` and its parameters. The merged scopes write
/// `+t.account_id` so SQLite walks the date index rather than the account's.
fn scope_condition(scope: &Scope) -> (String, Vec<Value>) {
    let accounts = "+t.account_id IN (SELECT value FROM json_each(?))";
    match scope {
        Scope::Unified { accounts: ids } => (
            format!("{accounts} AND {IN_INBOX}"),
            vec![Value::Text(ids_json(ids))],
        ),
        Scope::Inbox { account_id } => (
            format!("t.account_id = ? AND {IN_INBOX}"),
            vec![Value::Integer(*account_id)],
        ),
        Scope::Folder {
            account_id,
            folder_id,
        } => (
            format!("t.account_id = ? AND {IN_FOLDER}"),
            vec![Value::Integer(*account_id), Value::Integer(*folder_id)],
        ),
        // `threads.is_starred` is the roll-up of "a message is starred" (rollup.rs).
        Scope::Starred { accounts: ids } => (
            format!("{accounts} AND t.is_starred = 1"),
            vec![Value::Text(ids_json(ids))],
        ),
        Scope::Kind {
            accounts: ids,
            kind,
        } => (
            format!("{accounts} AND {IN_KIND}"),
            vec![
                Value::Text(ids_json(ids)),
                Value::Text(kind.as_str().to_string()),
                Value::Text(ids_json(ids)),
            ],
        ),
        Scope::Snoozed { accounts: ids } => (
            format!("{accounts} AND {HELD}"),
            vec![Value::Text(ids_json(ids))],
        ),
        // No folder join at all: any folder of these accounts.
        Scope::All { accounts: ids } => (accounts.to_string(), vec![Value::Text(ids_json(ids))]),
    }
}

/// How a `PageAt` reads: the keyset condition, its key, and the direction.
pub(super) fn direction(at: PageAt) -> (Option<(&'static str, ThreadKey)>, bool) {
    match at {
        PageAt::First => (None, true),
        PageAt::Last => (None, false),
        PageAt::Older(k) => (Some(("(t.last_date, t.id) < (?, ?)", k)), true),
        PageAt::Newer(k) => (Some(("(t.last_date, t.id) > (?, ?)", k)), false),
        PageAt::From(k) => (Some(("(t.last_date, t.id) <= (?, ?)", k)), true),
    }
}

/// The page's `SELECT` over the `where_clause` text `base`, and its parameters in order.
pub(super) fn rows_statement(
    base: &str,
    params: &[Value],
    keyset: Option<(&str, ThreadKey)>,
    descending: bool,
) -> (String, Vec<Value>) {
    let order = if descending { "DESC" } else { "ASC" };
    let mut values = params.to_vec();
    let mut condition = base.to_string();
    if let Some((sql, key)) = keyset {
        condition = format!("{condition} AND {sql}");
        values.extend([Value::Integer(key.last_date), Value::Integer(key.thread_id)]);
    }
    values.push(int(PAGE + 1));
    let sql = format!(
        "SELECT {THREAD_COLUMNS} FROM threads t WHERE {condition}
         ORDER BY t.last_date {order}, t.id {order} LIMIT ?"
    );
    (sql, values)
}

impl Store {
    /// One page of `query`'s threads at `at`, newest first.
    pub fn thread_page(&self, query: &ListQuery, at: PageAt) -> Result<ThreadPage, StoreError> {
        if query.account == AccountFilter::Nobody {
            return Ok(ThreadPage::default());
        }
        let (base, params) = where_clause(query);
        let (keyset, descending) = direction(at);
        let mut rows = self.thread_rows(&base, &params, keyset, descending)?;
        let more = rows.len() > PAGE;
        rows.truncate(PAGE);
        if !descending {
            rows.reverse();
        }
        let anchor = keyset.map(|(_, key)| key);
        let page = if descending {
            let top = rows.first().map(|r| r.key).or(anchor);
            let newer = match top {
                Some(key) if at != PageAt::First => self.any_beyond(&base, &params, key, ">")?,
                _ => false,
            };
            ThreadPage {
                rows,
                older: more,
                newer,
            }
        } else {
            let bottom = rows.last().map(|r| r.key).or(anchor);
            let older = match bottom {
                Some(key) if at != PageAt::Last => self.any_beyond(&base, &params, key, "<")?,
                _ => false,
            };
            ThreadPage {
                rows,
                older,
                newer: more,
            }
        };
        Ok(page)
    }

    fn thread_rows(
        &self,
        base: &str,
        params: &[Value],
        keyset: Option<(&str, ThreadKey)>,
        descending: bool,
    ) -> Result<Vec<ThreadRow>, StoreError> {
        let (sql, values) = rows_statement(base, params, keyset, descending);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), thread_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Whether a thread of the query sorts beyond `key` (`>`: newer, `<`: older).
    fn any_beyond(
        &self,
        base: &str,
        params: &[Value],
        key: ThreadKey,
        op: &str,
    ) -> Result<bool, StoreError> {
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM threads t WHERE {base} AND (t.last_date, t.id) {op} (?, ?))"
        );
        let mut values = params.to_vec();
        values.extend([Value::Integer(key.last_date), Value::Integer(key.thread_id)]);
        Ok(self
            .conn
            .query_row(&sql, params_from_iter(values), |r| r.get(0))?)
    }

    /// The thread's messages (the newest `MAX_THREAD_MESSAGES`, oldest first) with their
    /// bodies, links and attachments; `None` when there is no such thread.
    pub fn thread_detail(&self, thread_id: i64) -> Result<Option<ThreadDetail>, StoreError> {
        let head = self
            .conn
            .query_row(
                "SELECT account_id, subject, message_count FROM threads WHERE id = ?1",
                params![thread_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((account_id, subject, total)) = head else {
            return Ok(None);
        };
        let mut messages = self.thread_messages(thread_id)?;
        messages.reverse();
        self.attach(&mut messages)?;
        let older_left_out = usize::try_from(total)
            .unwrap_or(0)
            .saturating_sub(messages.len());
        Ok(Some(ThreadDetail {
            thread_id,
            account_id,
            subject,
            messages,
            older_left_out,
            snoozed_until: super::snoozes::held_until(&self.conn, thread_id)?,
        }))
    }

    fn thread_messages(&self, thread_id: i64) -> Result<Vec<MessageView>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.remote_id, m.from_json, m.to_json, m.cc_json, m.subject, m.date,
                    m.internal_date, m.is_read, m.is_starred, m.snippet,
                    substr(m.body_text, 1, ?2), length(m.body_text) > ?2,
                    m.body_fetched_at IS NOT NULL, m.links, m.body_html IS NOT NULL,
                    m.vouched_domains
             FROM messages m WHERE m.thread_id = ?1
             ORDER BY m.internal_date DESC, m.id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                thread_id,
                int(MAX_SHOWN_BODY_CHARS),
                int(MAX_THREAD_MESSAGES)
            ],
            message_view,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every attachment the store lists for that account's message, in `attachments.id`
    /// order; empty when the account has no such message. The only source of a part's
    /// filename, type and size for `attachment.save` and `attachment.open`: Graph's `$value`
    /// answers bytes with no metadata.
    ///
    /// # Errors
    /// The store could not be read.
    pub fn listed_attachments(
        &self,
        account_id: i64,
        remote: &str,
    ) -> Result<Vec<AttachmentView>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT a.remote_part_id, a.filename, a.mime_type, a.size
             FROM attachments a JOIN messages m ON m.id = a.message_id
             WHERE m.account_id = ?1 AND m.remote_id = ?2 ORDER BY a.id",
        )?;
        let rows = stmt.query_map(params![account_id, remote], |r| {
            Ok(AttachmentView {
                part_id: r.get(0)?,
                filename: r.get(1)?,
                mime_type: r.get(2)?,
                size: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The part of that account's message whose Content-ID is `content_id`, the first in
    /// `attachments.id` order; `None` when the store lists none.
    ///
    /// # Errors
    /// The store could not be read.
    pub fn inline_part(
        &self,
        account_id: i64,
        remote: &str,
        content_id: &str,
    ) -> Result<Option<AttachmentView>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT a.remote_part_id, a.filename, a.mime_type, a.size
                 FROM attachments a JOIN messages m ON m.id = a.message_id
                 WHERE m.account_id = ?1 AND m.remote_id = ?2 AND a.content_id = ?3
                 ORDER BY a.id LIMIT 1",
                params![account_id, remote, content_id],
                |r| {
                    Ok(AttachmentView {
                        part_id: r.get(0)?,
                        filename: r.get(1)?,
                        mime_type: r.get(2)?,
                        size: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Whether that account's message is stored with HTML naming a `cid:` address (in any
    /// case): only then can a picture it lacks be found by fetching its body again.
    ///
    /// # Errors
    /// The store could not be read.
    pub fn names_cid(&self, account_id: i64, remote: &str) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE account_id = ?1 AND remote_id = ?2
                 AND instr(lower(body_html), 'cid:') > 0",
                params![account_id, remote],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    fn attach(&self, messages: &mut [MessageView]) -> Result<(), StoreError> {
        let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
        let mut stmt = self.conn.prepare(
            "SELECT message_id, remote_part_id, filename, mime_type, size FROM attachments
             WHERE message_id IN (SELECT value FROM json_each(?1)) ORDER BY id",
        )?;
        let mut by_message: HashMap<i64, Vec<AttachmentView>> = HashMap::new();
        let rows = stmt.query_map(params![ids_json(&ids)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                AttachmentView {
                    part_id: r.get(1)?,
                    filename: r.get(2)?,
                    mime_type: r.get(3)?,
                    size: r.get(4)?,
                },
            ))
        })?;
        for row in rows {
            let (message, attachment) = row?;
            by_message.entry(message).or_default().push(attachment);
        }
        for message in messages {
            message.attachments = by_message.remove(&message.id).unwrap_or_default();
        }
        Ok(())
    }

    /// One message's HTML part; `None` when there is no such message.
    pub fn message_html(&self, message_id: i64) -> Result<Option<HtmlBody>, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT body_fetched_at IS NOT NULL, body_html FROM messages WHERE id = ?1",
                params![message_id],
                |r| Ok((r.get::<_, bool>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        Ok(row.map(|(fetched, html)| match (fetched, html) {
            (false, _) => HtmlBody::NotFetched,
            (true, None) => HtmlBody::Absent,
            (true, Some(html)) => HtmlBody::Html(html),
        }))
    }

    /// The store id of each address (`None`: no row yet).
    pub fn account_ids(&self, addresses: &[String]) -> Result<Vec<Option<i64>>, StoreError> {
        addresses.iter().map(|a| self.account_id(a)).collect()
    }
}

fn message_view(r: &rusqlite::Row<'_>) -> rusqlite::Result<MessageView> {
    let id: i64 = r.get(0)?;
    let (from, to, cc): (String, String, String) = (r.get(2)?, r.get(3)?, r.get(4)?);
    let text: Option<String> = r.get(11)?;
    let cut: Option<bool> = r.get(12)?;
    let body = match (r.get::<_, bool>(13)?, text) {
        (false, _) => BodyState::NotFetched,
        (true, Some(text)) if text == UNREADABLE_BODY => BodyState::Unreadable,
        (true, text) => BodyState::Text {
            text: text.unwrap_or_default(),
            cut: cut.unwrap_or(false),
        },
    };
    let links: Option<String> = r.get(14)?;
    Ok(MessageView {
        id,
        remote_id: r.get(1)?,
        from: mailboxes(&from),
        to: mailboxes(&to),
        cc: mailboxes(&cc),
        subject: r.get(5)?,
        date: r.get(6)?,
        internal_date: r.get(7)?,
        is_read: r.get(8)?,
        is_starred: r.get(9)?,
        snippet: r.get(10)?,
        body,
        links: links.map(|json| read_links(id, &json)).unwrap_or_default(),
        attachments: Vec::new(),
        has_html: r.get(15)?,
        vouched_domains: r
            .get::<_, Option<String>>(16)?
            .map(|json| read_vouched(id, &json))
            .unwrap_or_default(),
    })
}

/// The vouched domains column, at most `MAX_VOUCHED`. An unreadable one (a hand-edited store)
/// is logged by message id and read as none: the sender's pictures then wait to be asked for.
fn read_vouched(id: i64, json: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(json) {
        Ok(mut domains) => {
            domains.truncate(MAX_VOUCHED);
            domains
        }
        Err(_) => {
            tracing::warn!(message = id, "a message's vouched domains were unreadable");
            Vec::new()
        }
    }
}

/// The links column, at most `MAX_LINKS`. An unreadable one (a hand-edited store) is logged
/// by message id and read as none: the markers then show as plain text.
pub(super) fn read_links(id: i64, json: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(json) {
        Ok(mut links) => {
            links.truncate(MAX_LINKS);
            links
        }
        Err(_) => {
            tracing::warn!(
                message = id,
                "a message's links column did not parse; showing none"
            );
            Vec::new()
        }
    }
}
