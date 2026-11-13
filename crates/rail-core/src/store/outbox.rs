//! The outbox: actions waiting for the worker, in one table. Every write runs in one
//! `IMMEDIATE` transaction on the writer's connection (`Store::outbox_write`); the reads serve
//! the worker, the operations and the status. Rows hold ids, flags and Rail's error words only.
//! A draft row (`send`, `push_draft`, `delete_draft`) names no threads: it is keyed by its
//! `draft_id`, and a `send` row moves its draft between `editing` and `queued`.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, params};

use super::apply_local::{self, Prior};
use super::drafts::{DraftSaved, DraftWrite, ListedFile};
use super::outbox_drafts::{draft_remote, draft_write, enqueue_draft, insert_delete_draft};
use super::snoozes::{self, SnoozeNew};
use super::{Store, StoreError, rollup};
use crate::ops::{Action, DraftId, OutboxId, RowToken};
use crate::provider::DraftIds;

/// Attempts after which a transiently failing row gives up.
pub const MAX_ATTEMPTS: u32 = 20;
/// Failed rows `failed_rows` reads at most (one account's, listed again on a connect).
pub const MAX_FAILED_ROWS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    Pending,
    Inflight,
    Failed,
}

impl OutboxState {
    fn parse(text: &str) -> Self {
        match text {
            "inflight" => Self::Inflight,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// One row. `last_error` is Rail's words with the provider's text, never mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: OutboxId,
    pub account_id: i64,
    pub action: Action,
    pub thread_ids: Vec<i64>,
    pub state: OutboxState,
    pub attempts: u32,
    pub not_before: i64,
    pub last_error: Option<String>,
    pub(crate) reversal: Vec<Prior>,
    pub(crate) folder_remotes: BTreeMap<i64, String>,
    /// The draft of a draft row.
    pub draft_id: Option<DraftId>,
    /// The token the row was queued with: undo and retry must name it.
    pub client_token: String,
    /// When an SMTP relay accepted this row's message, if it has. A resend after a Sent-copy
    /// failure, or after a restart, must not submit it again.
    pub relay_accepted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAction {
    pub account_id: i64,
    pub action: Action,
    /// Checked by the caller: this account's threads, deduplicated. The enqueue keeps only
    /// those the store still holds for the account when it writes.
    pub threads: Vec<i64>,
    pub not_before: i64,
    pub client_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Back to `pending` until `retry_at`, unless this was attempt `MAX_ATTEMPTS`.
    Transient { error: String, retry_at: i64 },
    /// `failed`, its local change reverted.
    Permanent(String),
}

/// One outbox write for the writer thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboxWrite {
    Enqueue(NewAction),
    Claim {
        account_id: i64,
    },
    Complete {
        id: OutboxId,
        /// The provider's send consumed its draft (Graph): a sent draft leaves no delete of it.
        draft_consumed: bool,
    },
    Fail {
        id: OutboxId,
        failure: Failure,
    },
    /// Inflight back to pending, no attempt counted (the account needs a login).
    Release {
        id: OutboxId,
    },
    /// The SMTP relay accepted this row's message: keep it, so a retry only finishes the
    /// local Sent copy. The row itself stays inflight.
    RelayAccepted {
        id: OutboxId,
    },
    /// Pending rows whose token matches go; any other is too late.
    Undo {
        rows: Vec<RowToken>,
    },
    /// A failed row whose token matches is queued again.
    Retry {
        id: OutboxId,
        client_token: String,
    },
    /// Every inflight row of the account back to pending (daemon start).
    Recover {
        account_id: i64,
    },
    /// The draft is about to be handed to the provider's send: stamp `send_attempted_at` (kept
    /// once set) so every later send of it looks the message up first. `remote` is the
    /// provider's draft that send sends (Graph), kept as the draft's remote ids; `listed`, the
    /// sizes its save says the provider lists for the draft's files, recorded on them.
    DraftSendAttempted {
        draft: DraftId,
        remote: Option<DraftIds>,
        listed: Vec<ListedFile>,
    },
    /// `draft.save`'s write: every store write goes through the one writer.
    DraftSave {
        write: Box<DraftWrite>,
        client_token: String,
    },
    /// `draft.discard`'s delete, refused while the draft is being sent or pushed; a draft
    /// Gmail holds leaves a Gmail delete.
    DraftDelete(DraftId),
    /// A push landed: the draft keeps the provider's ids and records the sizes it lists for
    /// the draft's files (`listed`), and the row goes.
    CompletePush {
        id: OutboxId,
        draft: DraftId,
        remote_draft_id: String,
        remote_message_id: String,
        listed: Vec<ListedFile>,
    },
    /// The import of a Gmail draft: one local draft per Gmail message, keeping Gmail's ids.
    DraftImport {
        write: Box<DraftWrite>,
        remote_draft_id: String,
        remote_message_id: String,
        client_token: String,
    },
    /// Snooze threads: archive them now and record when they go back (`store::snoozes`).
    Snooze(SnoozeNew),
    /// Put these threads' snoozes back now.
    Unsnooze {
        account_id: i64,
        threads: Vec<i64>,
    },
    /// Put back every snooze of the account due by now.
    Wake {
        account_id: i64,
    },
}

/// What `draft.discard`'s delete did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftDeleteOutcome {
    /// Gone (or gone already); a Gmail delete is queued when Gmail held it.
    Deleted,
    /// The draft is being sent: nothing changed.
    BeingSent,
    /// A push of the draft is in flight: nothing changed; try again once it lands.
    BeingPushed,
}

/// An account's outbox rows: waiting (inflight ones too), failed, `send` rows in flight and
/// `push_draft` rows in flight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxCounts {
    pub pending: u64,
    pub failed: u64,
    pub sending: u64,
    pub saving: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboxOutcome {
    /// The row ids, and how many of the threads the store holds; no row when it holds none.
    Enqueued {
        rows: Vec<OutboxId>,
        applied: usize,
    },
    Claimed(Option<OutboxRow>),
    Done,
    Failed {
        gave_up: bool,
        row: Option<OutboxRow>,
    },
    Undone {
        undone: Vec<OutboxId>,
        too_late: Vec<OutboxId>,
    },
    Retried(bool),
    /// The rows moved back to pending: the worker looks a `send` among them up before sending.
    Recovered(Vec<OutboxId>),
    DraftSaved(DraftSaved),
    DraftDeleted(DraftDeleteOutcome),
    /// The archive's rows (none when every thread was snoozed already) and how many threads are
    /// snoozed now.
    Snoozed {
        rows: Vec<OutboxId>,
        snoozed: usize,
    },
    Unsnoozed {
        returned: usize,
    },
    /// The threads (the store's ids) a wake put back.
    Woke {
        threads: Vec<i64>,
    },
}

const COLUMNS: &str = "id, account_id, payload, thread_ids, state, attempts, not_before, \
    last_error, reversal, draft_id, client_token, folder_remotes, relay_accepted_at";

fn read_row(r: &Row<'_>) -> rusqlite::Result<OutboxRow> {
    let json = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    let action: Action = serde_json::from_str(&json(2)?).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(OutboxRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        action,
        // Rail's own JSON; an unreadable list (a hand-edited store) acts on nothing.
        thread_ids: serde_json::from_str(&json(3)?).unwrap_or_default(),
        state: OutboxState::parse(&json(4)?),
        attempts: r.get(5)?,
        not_before: r.get(6)?,
        last_error: r.get(7)?,
        reversal: serde_json::from_str(&json(8)?).unwrap_or_default(),
        draft_id: r.get(9)?,
        client_token: r.get(10)?,
        folder_remotes: serde_json::from_str(&json(11)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e))
        })?,
        relay_accepted_at: r.get(12)?,
    })
}

pub(super) fn one(tx: &Transaction, id: OutboxId) -> Result<Option<OutboxRow>, StoreError> {
    Ok(tx
        .query_row(
            &format!("SELECT {COLUMNS} FROM outbox WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?)
}

pub(super) fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

/// Rows (live, and `also` whatever their state) whose threads meet `threads`, oldest first.
pub(super) fn live_rows_touching(
    tx: &Transaction,
    threads: &BTreeSet<i64>,
    also: &[OutboxId],
) -> Result<Vec<OutboxRow>, StoreError> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM outbox o
         WHERE (o.state IN ('pending', 'inflight') OR o.id IN (SELECT value FROM json_each(?2)))
           AND EXISTS (SELECT 1 FROM json_each(o.thread_ids) t
                       WHERE t.value IN (SELECT value FROM json_each(?1)))
         ORDER BY o.id"
    ))?;
    let rows = stmt.query_map(params![json(threads), json(&also)], read_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Every row, whatever its state, whose threads meet `threads`.
pub(super) fn rows_touching(
    tx: &Transaction,
    threads: &BTreeSet<i64>,
) -> Result<Vec<OutboxRow>, StoreError> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM outbox o WHERE EXISTS (SELECT 1 FROM json_each(o.thread_ids) t
           WHERE t.value IN (SELECT value FROM json_each(?1))) ORDER BY o.id"
    ))?;
    let rows = stmt.query_map(params![json(threads)], read_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub(super) fn save_threads_and_reversal(
    tx: &Transaction,
    id: OutboxId,
    threads: &[i64],
    reversal: &[Prior],
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET thread_ids = ?2, reversal = ?3 WHERE id = ?1",
        params![id, json(&threads), json(&reversal)],
    )?;
    Ok(())
}

/// The ids of `threads` the store holds for `account`, in the order given.
fn held(tx: &Transaction, account: i64, threads: &[i64]) -> Result<Vec<i64>, StoreError> {
    let mut stmt = tx.prepare(
        "SELECT id FROM threads WHERE account_id = ?1 AND id IN (SELECT value FROM json_each(?2))",
    )?;
    let found: BTreeSet<i64> = stmt
        .query_map(params![account, json(&threads)], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(threads
        .iter()
        .copied()
        .filter(|t| found.contains(t))
        .collect())
}

/// Add the row and apply its change locally, in the caller's transaction, for the threads the
/// store still holds for the account (a sync batch may have deleted one since the operation
/// checked them). A token already in the outbox answers with that row and changes nothing.
/// `None` when none of the threads is held any more: nothing is written.
pub(crate) fn enqueue(
    tx: &Transaction,
    new: &NewAction,
) -> Result<Option<(OutboxId, usize)>, StoreError> {
    let threads = held(tx, new.account_id, &new.threads)?;
    let known: Option<OutboxId> = tx
        .query_row(
            "SELECT id FROM outbox WHERE client_token = ?1",
            [&new.client_token],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = known {
        return Ok(Some((id, threads.len())));
    }
    if let Some(draft) = new.action.draft_id() {
        return enqueue_draft(tx, new, draft);
    }
    if threads.is_empty() {
        return Ok(None);
    }
    let remotes = super::action_folders::capture(tx, new.account_id, &new.action)?;
    let reversal = apply_local::apply(tx, new.account_id, &new.action, &threads)?;
    snoozes::left_inbox(tx, new.account_id, &new.action, &threads)?;
    tx.execute(
        "INSERT INTO outbox (account_id, kind, payload, state, client_token, not_before, thread_ids, reversal, folder_remotes)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8)",
        params![
            new.account_id,
            new.action.kind_column(),
            json(&new.action),
            new.client_token,
            new.not_before,
            json(&threads),
            json(&reversal),
            json(&remotes)
        ],
    )?;
    let id = tx.last_insert_rowid();
    rollup::refresh(tx, &threads.iter().copied().collect())?;
    Ok(Some((id, threads.len())))
}

/// The draft's state, when it exists (and is `account`'s, when given).
pub(super) fn draft_state(
    tx: &Transaction,
    draft: DraftId,
    account: Option<i64>,
) -> Result<Option<String>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT state FROM drafts WHERE id = ?1 AND (?2 IS NULL OR account_id = ?2)",
            params![draft, account],
            |r| r.get(0),
        )
        .optional()?)
}

/// The draft's `editing` or `queued`.
pub(super) fn set_draft_state(
    tx: &Transaction,
    draft: DraftId,
    state: &str,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE drafts SET state = ?2 WHERE id = ?1",
        params![draft, state],
    )?;
    Ok(())
}

/// The draft of `row` when it is a `send` row.
fn sent_draft(row: &OutboxRow) -> Option<DraftId> {
    matches!(row.action, Action::Send { .. })
        .then_some(row.draft_id)
        .flatten()
}

/// The oldest due pending row of `account`, now inflight. Per-thread FIFO: a row waits (even
/// when due) while an older live row shares a thread with it, so Gmail sees one thread's
/// actions in the order the user made them; rows of one draft wait the same way. Done in
/// SQL: one bounded statement, no row loop.
pub(crate) fn claim_next(
    tx: &Transaction,
    account: i64,
    now: i64,
) -> Result<Option<OutboxRow>, StoreError> {
    let id: Option<OutboxId> = tx
        .query_row(
            "UPDATE outbox SET state = 'inflight', inflight_at = ?2
             WHERE id = (SELECT o.id FROM outbox o
                         WHERE o.account_id = ?1 AND o.state = 'pending' AND o.not_before <= ?2
                           AND NOT EXISTS (
                             SELECT 1 FROM outbox p
                             WHERE p.id < o.id AND p.state IN ('pending', 'inflight')
                               AND (EXISTS (SELECT 1 FROM json_each(p.thread_ids) pt
                                            WHERE pt.value IN (SELECT value FROM json_each(o.thread_ids)))
                                    OR (p.draft_id IS NOT NULL AND p.draft_id = o.draft_id)))
                         ORDER BY o.id LIMIT 1)
             RETURNING id",
            params![account, now],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => one(tx, id),
        None => Ok(None),
    }
}

/// The row is done: it leaves the outbox, and a sent draft leaves the store with every other
/// row of it (a failed earlier send among them), so none can act on the draft again. A sent
/// draft the provider holds leaves a delete of that copy, due `now`, in this transaction,
/// unless the send consumed it (`draft_consumed`: Graph sends the draft itself).
pub(crate) fn complete(
    tx: &Transaction,
    id: OutboxId,
    draft_consumed: bool,
    now: i64,
) -> Result<(), StoreError> {
    let sent: Option<DraftId> = tx
        .query_row(
            "SELECT draft_id FROM outbox WHERE id = ?1 AND kind = 'send'",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(draft) = sent {
        if !draft_consumed && let Some((account_id, Some(remote))) = draft_remote(tx, draft)? {
            let token = format!("after-send-{id}");
            insert_delete_draft(tx, account_id, draft, &remote, &token, now)?;
        }
        tx.execute(
            "DELETE FROM outbox WHERE draft_id = ?1 AND id != ?2",
            params![draft, id],
        )?;
        tx.execute("DELETE FROM drafts WHERE id = ?1", [draft])?;
    }
    snoozes::archive_sent(tx, id)?;
    tx.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
    Ok(())
}

pub(crate) fn release(tx: &Transaction, id: OutboxId) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET state = 'pending', inflight_at = NULL WHERE id = ?1 AND state = 'inflight'",
        [id],
    )?;
    Ok(())
}

/// Stamp relay acceptance once; a later retry must see it.
pub(crate) fn relay_accepted(tx: &Transaction, id: OutboxId, now: i64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET relay_accepted_at = COALESCE(relay_accepted_at, ?2) WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

/// Record a failure: back to pending with the backoff, or (permanent, or the last attempt)
/// `failed` with its local change reverted. Whether it gave up, and the row as it is now.
pub(crate) fn fail(
    tx: &Transaction,
    id: OutboxId,
    failure: &Failure,
) -> Result<(bool, Option<OutboxRow>), StoreError> {
    let Some(row) = one(tx, id)? else {
        return Ok((false, None));
    };
    let attempts = row.attempts.saturating_add(1);
    let (error, retry_at) = match failure {
        Failure::Transient { error, retry_at } if attempts < MAX_ATTEMPTS => {
            (error.clone(), Some(*retry_at))
        }
        Failure::Transient { error, .. } => (
            format!("gave up after {MAX_ATTEMPTS} attempts: {error}"),
            None,
        ),
        Failure::Permanent(error) => (error.clone(), None),
    };
    match retry_at {
        Some(at) => {
            tx.execute(
                "UPDATE outbox SET state = 'pending', attempts = ?2, last_error = ?3, not_before = ?4,
                   inflight_at = NULL WHERE id = ?1",
                params![id, attempts, error, at],
            )?;
        }
        None => {
            apply_local::restack(tx, &row.thread_ids.iter().copied().collect(), &[id])?;
            if let Some(draft) = sent_draft(&row) {
                set_draft_state(tx, draft, "editing")?;
            }
            tx.execute(
                "UPDATE outbox SET state = 'failed', attempts = ?2, last_error = ?3, inflight_at = NULL
                 WHERE id = ?1",
                params![id, attempts, error],
            )?;
        }
    }
    Ok((retry_at.is_none(), one(tx, id)?))
}

/// Remove the rows still pending whose token matches and revert their change (a sent draft
/// goes back to `editing`); the others are too late.
pub(crate) fn remove_pending(
    tx: &Transaction,
    rows: &[RowToken],
) -> Result<(Vec<OutboxId>, Vec<OutboxId>), StoreError> {
    let (mut undone, mut too_late, mut threads) = (Vec::new(), Vec::new(), BTreeSet::new());
    for RowToken {
        row: id,
        client_token,
    } in rows
    {
        match one(tx, *id)? {
            Some(row) if row.state == OutboxState::Pending && &row.client_token == client_token => {
                threads.extend(row.thread_ids.iter().copied());
                if let Some(draft) = sent_draft(&row) {
                    set_draft_state(tx, draft, "editing")?;
                }
                undone.push(*id);
            }
            _ => too_late.push(*id),
        }
    }
    apply_local::restack(tx, &threads, &undone)?;
    // Deleted, not completed: an undone send keeps its draft.
    for id in &undone {
        tx.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
    }
    Ok((undone, too_late))
}

/// `failed` → `pending` now, its change applied again in order (a sent draft is `queued`
/// again). False for any other row, a token that is not the row's, or a `send` whose draft
/// is gone or not `editing`.
pub(crate) fn retry(
    tx: &Transaction,
    id: OutboxId,
    client_token: &str,
    now: i64,
) -> Result<bool, StoreError> {
    let Some(row) =
        one(tx, id)?.filter(|r| r.state == OutboxState::Failed && r.client_token == client_token)
    else {
        return Ok(false);
    };
    if let Some(draft) = sent_draft(&row) {
        if draft_state(tx, draft, Some(row.account_id))?.as_deref() != Some("editing") {
            return Ok(false);
        }
        set_draft_state(tx, draft, "queued")?;
    }
    tx.execute(
        "UPDATE outbox SET state = 'pending', attempts = 0, last_error = NULL, not_before = ?2,
           reversal = '[]' WHERE id = ?1",
        params![id, now],
    )?;
    apply_local::restack(tx, &row.thread_ids.iter().copied().collect(), &[])?;
    Ok(true)
}

/// Spec §3: a failed row is kept until it is retried or a sync writes server state for one of
/// its threads. Deletes the account's failed rows on `threads`; how many. A snooze's failed
/// return is the exception (#106): the server's state is the very archive it failed to undo, and
/// nothing else would bring the thread back, so it stays until it is retried, the thread is in
/// the inbox again by other means, or Rail takes the thread out again (`snoozes::left_inbox`).
pub(super) fn clear_failed(
    tx: &Transaction,
    account: i64,
    threads: &BTreeSet<i64>,
) -> Result<usize, StoreError> {
    if threads.is_empty() {
        return Ok(0);
    }
    Ok(tx.execute(
        concat!(
            "DELETE FROM outbox WHERE account_id = ?1 AND state = 'failed'
               AND EXISTS (SELECT 1 FROM json_each(outbox.thread_ids) t
                           WHERE t.value IN (SELECT value FROM json_each(?2)))
               AND NOT (",
            snoozes::return_still_owed!("outbox"),
            ")"
        ),
        params![account, json(threads)],
    )?)
}

/// The restart rule: inflight rows of `account` are sent again; their ids.
pub(crate) fn recover(tx: &Transaction, account: i64) -> Result<Vec<OutboxId>, StoreError> {
    let mut stmt = tx.prepare(
        "UPDATE outbox SET state = 'pending', inflight_at = NULL
         WHERE account_id = ?1 AND state = 'inflight' RETURNING id",
    )?;
    let ids = stmt.query_map([account], |r| r.get(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}

impl Store {
    /// One outbox write in one `IMMEDIATE` transaction.
    pub fn outbox_write(
        &mut self,
        write: &OutboxWrite,
        now: i64,
    ) -> Result<OutboxOutcome, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let outcome = match write {
            OutboxWrite::Enqueue(new) => {
                let (rows, applied) =
                    enqueue(&tx, new)?.map_or((Vec::new(), 0), |(id, n)| (vec![id], n));
                OutboxOutcome::Enqueued { rows, applied }
            }
            OutboxWrite::Claim { account_id } => {
                OutboxOutcome::Claimed(claim_next(&tx, *account_id, now)?)
            }
            OutboxWrite::Complete { id, draft_consumed } => {
                complete(&tx, *id, *draft_consumed, now)?;
                OutboxOutcome::Done
            }
            OutboxWrite::Fail { id, failure } => {
                let (gave_up, row) = fail(&tx, *id, failure)?;
                OutboxOutcome::Failed { gave_up, row }
            }
            OutboxWrite::Release { id } => {
                release(&tx, *id)?;
                OutboxOutcome::Done
            }
            OutboxWrite::RelayAccepted { id } => {
                relay_accepted(&tx, *id, now)?;
                OutboxOutcome::Done
            }
            OutboxWrite::Undo { rows } => {
                let (undone, too_late) = remove_pending(&tx, rows)?;
                OutboxOutcome::Undone { undone, too_late }
            }
            OutboxWrite::Retry { id, client_token } => {
                OutboxOutcome::Retried(retry(&tx, *id, client_token, now)?)
            }
            OutboxWrite::Recover { account_id } => {
                OutboxOutcome::Recovered(recover(&tx, *account_id)?)
            }
            OutboxWrite::DraftSendAttempted { .. }
            | OutboxWrite::DraftSave { .. }
            | OutboxWrite::DraftDelete(_)
            | OutboxWrite::CompletePush { .. }
            | OutboxWrite::DraftImport { .. } => draft_write(&tx, write, now)?,
            OutboxWrite::Snooze(new) => snoozes::snooze(&tx, new, now)?,
            OutboxWrite::Unsnooze {
                account_id,
                threads,
            } => snoozes::clear(&tx, *account_id, threads, now)?,
            OutboxWrite::Wake { account_id } => snoozes::wake(&tx, *account_id, now)?,
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// The account's rows by state: waiting counts inflight ones, sending is the inflight
    /// `send` rows, and saving the inflight `push_draft` rows.
    pub fn outbox_depth(&self, account_id: i64) -> Result<OutboxCounts, StoreError> {
        let counts: [i64; 4] = self.conn.query_row(
            "SELECT COALESCE(sum(state != 'failed'), 0), COALESCE(sum(state = 'failed'), 0),
               COALESCE(sum(state = 'inflight' AND kind = 'send'), 0),
               COALESCE(sum(state = 'inflight'
                            AND json_extract(payload, '$.kind') = 'push_draft'), 0)
             FROM outbox WHERE account_id = ?1",
            [account_id],
            |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]),
        )?;
        let count = |n: i64| u64::try_from(n).unwrap_or(0);
        Ok(OutboxCounts {
            pending: count(counts[0]),
            failed: count(counts[1]),
            sending: count(counts[2]),
            saving: count(counts[3]),
        })
    }

    /// The earliest `not_before` of a pending row that `claim_next` can take once due: rows
    /// held back by an older live row on a shared thread or draft are left out, so a worker sleeping
    /// until this time is not woken by a row it cannot claim.
    pub fn outbox_next_due(&self, account_id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self.conn.query_row(
            "SELECT min(o.not_before) FROM outbox o
             WHERE o.account_id = ?1 AND o.state = 'pending'
               AND NOT EXISTS (
                 SELECT 1 FROM outbox p
                 WHERE p.id < o.id AND p.state IN ('pending', 'inflight')
                   AND (EXISTS (SELECT 1 FROM json_each(p.thread_ids) pt
                                WHERE pt.value IN (SELECT value FROM json_each(o.thread_ids)))
                        OR (p.draft_id IS NOT NULL AND p.draft_id = o.draft_id)))",
            [account_id],
            |r| r.get(0),
        )?)
    }

    /// Live rows whose threads meet `thread_ids`.
    pub fn pending_for(&self, thread_ids: &[i64]) -> Result<Vec<OutboxRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM outbox o WHERE o.state IN ('pending', 'inflight')
             AND EXISTS (SELECT 1 FROM json_each(o.thread_ids) t
                         WHERE t.value IN (SELECT value FROM json_each(?1))) ORDER BY o.id"
        ))?;
        let rows = stmt.query_map(params![json(&thread_ids)], read_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The draft's live (pending or inflight) rows, oldest first.
    pub fn pending_rows_for_draft(&self, draft: DraftId) -> Result<Vec<OutboxRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM outbox WHERE draft_id = ?1
             AND state IN ('pending', 'inflight') ORDER BY id"
        ))?;
        let rows = stmt.query_map([draft], read_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One row, whatever its state.
    pub(crate) fn outbox_row(&self, id: OutboxId) -> Result<Option<OutboxRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM outbox WHERE id = ?1"),
                [id],
                read_row,
            )
            .optional()?)
    }

    /// The account's failed rows, oldest first, at most `MAX_FAILED_ROWS`.
    pub fn failed_rows(&self, account_id: i64) -> Result<Vec<OutboxRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM outbox WHERE account_id = ?1 AND state = 'failed'
             ORDER BY id LIMIT ?2"
        ))?;
        let limit = i64::try_from(MAX_FAILED_ROWS).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![account_id, limit], read_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The provider ids of the threads of `threads` the store still holds for the account.
    pub fn thread_remotes(
        &self,
        account_id: i64,
        threads: &[i64],
    ) -> Result<Vec<String>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT remote_id FROM threads WHERE account_id = ?1
             AND id IN (SELECT value FROM json_each(?2)) ORDER BY id",
        )?;
        let rows = stmt.query_map(params![account_id, json(&threads)], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// True when every id of `labels` is one of the account's labels (kind `label` or `user`).
    pub fn user_labels(&self, account_id: i64, labels: &[i64]) -> Result<bool, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM folders WHERE account_id = ?1 AND kind IN ('label', 'user')
             AND id IN (SELECT value FROM json_each(?2))",
            params![account_id, json(&labels)],
            |r| r.get(0),
        )?;
        let distinct: BTreeSet<i64> = labels.iter().copied().collect();
        Ok(usize::try_from(n).unwrap_or(0) == distinct.len())
    }

    /// The provider id of one of the account's folders.
    pub fn folder_remote(
        &self,
        account_id: i64,
        folder_id: i64,
    ) -> Result<Option<String>, StoreError> {
        super::targets::folder_remote_in(&self.conn, account_id, folder_id)
    }
}
