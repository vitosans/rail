//! `Store::apply`: one sync batch in one transaction. Only the writer thread calls it.

use std::collections::{BTreeSet, HashSet};

use rusqlite::types::ToSql;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::snoozes::NOT_SNOOZED;
use super::{Store, StoreError, rollup};
use crate::provider::{Body, Envelope, Mailbox, Relabel, RemoteFolder, RemoteId};
use crate::sync::batch::{Batch, BatchOutcome, Origin, Prune, ResumeToken, StateUpdate, Sweep};

const UPSERT: &str = "INSERT INTO messages (account_id, thread_id, remote_id, message_id,
    in_reply_to, references_ids, subject, from_json, to_json, cc_json, bcc_json, date, snippet,
    is_read, is_starred, is_draft, size, internal_date, origin, searched_at, synced_at,
    from_text, to_text, reply_to_json)
  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
    ?20, ?21, ?22, ?23, ?24)
  ON CONFLICT(account_id, remote_id) DO UPDATE SET
    thread_id = excluded.thread_id, message_id = excluded.message_id,
    in_reply_to = excluded.in_reply_to, references_ids = excluded.references_ids,
    subject = excluded.subject, from_json = excluded.from_json, to_json = excluded.to_json,
    cc_json = excluded.cc_json, bcc_json = excluded.bcc_json, date = excluded.date,
    snippet = excluded.snippet, is_read = excluded.is_read, is_starred = excluded.is_starred,
    is_draft = excluded.is_draft, size = excluded.size, internal_date = excluded.internal_date,
    origin = CASE WHEN excluded.origin = 'sync' THEN 'sync' ELSE messages.origin END,
    searched_at = COALESCE(excluded.searched_at, messages.searched_at),
    synced_at = COALESCE(excluded.synced_at, messages.synced_at),
    from_text = excluded.from_text, to_text = excluded.to_text,
    reply_to_json = excluded.reply_to_json";

/// `[]`-style JSON of Rail's own types; serialising them cannot fail.
fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

/// Mailboxes as the full-text index holds them: `Name address` for each, separated by single
/// spaces; only the address when the name is missing or empty. Nothing is escaped (the FTS
/// tokenizer splits on controls and punctuation). Migration 0005 computes the same text in
/// SQL for rows written before it; the two must stay byte for byte equal.
pub(crate) fn address_text<'a>(mailboxes: impl IntoIterator<Item = &'a Mailbox>) -> String {
    let parts: Vec<String> = mailboxes
        .into_iter()
        .map(|m| match m.name.as_deref() {
            None | Some("") => m.address.clone(),
            Some(name) => format!("{name} {}", m.address),
        })
        .collect();
    parts.join(" ")
}

impl Store {
    /// Apply `batch` in one `IMMEDIATE` transaction, recomputing rollups for the threads it
    /// touched. Any failure rolls all of it back.
    pub fn apply(&mut self, batch: &Batch, now: i64) -> Result<BatchOutcome, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = batch.account_id;
        let mut touched = BTreeSet::new();
        let mut outcome = BatchOutcome::default();
        if let Some(folders) = &batch.folders {
            replace_folders(&tx, account, folders)?;
        }
        for envelope in &batch.upserts {
            upsert(&tx, account, envelope, batch.origin, now, &mut touched)?;
        }
        outcome.upserted = batch.upserts.len();
        for change in &batch.relabels {
            relabel(&tx, account, change, &mut touched)?;
        }
        outcome.deleted = delete(&tx, account, &batch.deletes, &mut touched)?;
        // What the server said about these threads settles their failed actions (a body
        // fetched for the reader, a sweep or a prune does not).
        outcome.failed_cleared = super::outbox::clear_failed(&tx, account, &touched)?;
        for (id, body) in &batch.bodies {
            store_body(&tx, account, id, body, now, &mut touched)?;
        }
        mark_searched(&tx, account, &batch.searched, now)?;
        if let Some(sweep) = batch.sweep {
            outcome.swept = sweep_unseen(&tx, account, sweep, &mut touched)?;
        }
        if let Some(prune) = batch.prune {
            outcome.pruned = prune_old(&tx, account, prune, &mut touched)?;
        }
        // Before the waiting actions are applied again: a snooze's own archive never hides it.
        outcome.snoozes_due = incoming_snoozes(&tx, batch, now)?;
        // The conflict rule: actions still waiting for Gmail stay on top of what it said.
        super::apply_local::reapply_live(&tx, account, &touched)?;
        super::snoozes::settle_woken(&tx, account, &touched)?;
        rollup::refresh(&tx, &touched)?;
        // A thread the rollup deleted leaves the outbox before its rowid can be reused.
        super::apply_local::forget_threads(&tx, &touched)?;
        update_state(&tx, account, &batch.state)?;
        #[cfg(test)]
        if batch.poison {
            return Err(StoreError::Sqlite(rusqlite::Error::InvalidQuery));
        }
        tx.commit()?;
        Ok(outcome)
    }
}

/// Mail the batch put in the inbox brings a snoozed thread back early: how many snoozes that
/// made due.
fn incoming_snoozes(tx: &Transaction, batch: &Batch, now: i64) -> Result<usize, StoreError> {
    let arrived: Vec<&str> = (batch.upserts.iter().map(|e| e.remote_id.as_str()))
        .chain(batch.relabels.iter().map(|r| r.remote_id.as_str()))
        .collect();
    super::snoozes::incoming(tx, batch.account_id, &arrived, now)
}

fn replace_folders(
    tx: &Transaction,
    account: i64,
    folders: &[RemoteFolder],
) -> Result<(), StoreError> {
    for folder in folders {
        tx.execute(
            "INSERT INTO folders (account_id, remote_id, name, kind) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, remote_id) DO UPDATE SET name = excluded.name, kind = excluded.kind",
            params![account, folder.remote_id, folder.name, folder.kind.as_str()],
        )?;
    }
    // A second pass: a child may be listed before its parent.
    for folder in folders {
        tx.execute(
            "UPDATE folders SET parent_id = (SELECT p.id FROM folders p
                 WHERE p.account_id = ?1 AND p.remote_id = ?3)
             WHERE account_id = ?1 AND remote_id = ?2",
            params![account, folder.remote_id, folder.parent],
        )?;
    }
    let keep: HashSet<&str> = folders.iter().map(|f| f.remote_id.as_str()).collect();
    let mut stmt = tx.prepare("SELECT id, remote_id FROM folders WHERE account_id = ?1")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let stale = rows
        .into_iter()
        .filter(|(_, remote)| !keep.contains(remote.as_str()));
    for (id, _) in stale {
        // `message_folders` rows cascade; the messages stay.
        tx.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
    }
    Ok(())
}

/// The folder row for `remote`; a label Rail has not seen yet gets a placeholder named after
/// its id, which the next labels step renames.
fn folder_id(tx: &Transaction, account: i64, remote: &str) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO folders (account_id, remote_id, name, kind) VALUES (?1, ?2, ?2, 'label')
         ON CONFLICT(account_id, remote_id) DO NOTHING",
        params![account, remote],
    )?;
    Ok(tx.query_row(
        "SELECT id FROM folders WHERE account_id = ?1 AND remote_id = ?2",
        params![account, remote],
        |r| r.get(0),
    )?)
}

/// `(message id, thread id)` of a stored message.
fn message_row(
    tx: &Transaction,
    account: i64,
    remote: &str,
) -> Result<Option<(i64, i64)>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT id, thread_id FROM messages WHERE account_id = ?1 AND remote_id = ?2",
            params![account, remote],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn thread_row(tx: &Transaction, account: i64, remote: &str) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO threads (account_id, remote_id) VALUES (?1, ?2)
         ON CONFLICT(account_id, remote_id) DO NOTHING",
        params![account, remote],
    )?;
    Ok(tx.query_row(
        "SELECT id FROM threads WHERE account_id = ?1 AND remote_id = ?2",
        params![account, remote],
        |r| r.get(0),
    )?)
}

fn upsert(
    tx: &Transaction,
    account: i64,
    env: &Envelope,
    origin: Origin,
    now: i64,
    touched: &mut BTreeSet<i64>,
) -> Result<(), StoreError> {
    if let Some((_, old_thread)) = message_row(tx, account, &env.remote_id)? {
        touched.insert(old_thread);
    }
    let thread = thread_row(tx, account, &env.thread_id)?;
    write_envelope(tx, account, thread, env, origin, now)?;
    super::addresses::remember(tx, env, now)?;
    let (message, _) = message_row(tx, account, &env.remote_id)?
        .ok_or(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
    tx.execute(
        "DELETE FROM message_folders WHERE message_id = ?1",
        params![message],
    )?;
    for remote in &env.folders {
        add_folder(tx, account, message, remote)?;
    }
    touched.insert(thread);
    Ok(())
}

/// Insert or update the `messages` row of `env` in `thread`.
fn write_envelope(
    tx: &Transaction,
    account: i64,
    thread: i64,
    env: &Envelope,
    origin: Origin,
    now: i64,
) -> Result<(), StoreError> {
    let (searched, synced) = match origin {
        Origin::Search => (Some(now), None),
        Origin::Sync => (None, Some(now)),
    };
    tx.execute(
        UPSERT,
        params![
            account,
            thread,
            env.remote_id,
            env.message_id,
            env.in_reply_to,
            json(&env.references),
            env.subject,
            json(&env.from),
            json(&env.to),
            json(&env.cc),
            json(&env.bcc),
            env.date,
            env.snippet,
            env.is_read,
            env.is_starred,
            env.is_draft,
            env.size,
            env.internal_date,
            origin.as_str(),
            searched,
            synced,
            address_text(&env.from),
            address_text(env.to.iter().chain(&env.cc).chain(&env.bcc)),
            json(&env.reply_to)
        ],
    )?;
    Ok(())
}

fn add_folder(
    tx: &Transaction,
    account: i64,
    message: i64,
    remote: &str,
) -> Result<(), StoreError> {
    let folder = folder_id(tx, account, remote)?;
    tx.execute(
        "INSERT OR IGNORE INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
        params![message, folder],
    )?;
    Ok(())
}

fn relabel(
    tx: &Transaction,
    account: i64,
    change: &Relabel,
    touched: &mut BTreeSet<i64>,
) -> Result<(), StoreError> {
    let Some((message, thread)) = message_row(tx, account, &change.remote_id)? else {
        return Ok(());
    };
    for remote in &change.add_folders {
        add_folder(tx, account, message, remote)?;
    }
    for remote in &change.remove_folders {
        tx.execute(
            "DELETE FROM message_folders WHERE message_id = ?1 AND folder_id =
               (SELECT id FROM folders WHERE account_id = ?2 AND remote_id = ?3)",
            params![message, account, remote],
        )?;
    }
    if let Some(read) = change.read {
        tx.execute(
            "UPDATE messages SET is_read = ?2 WHERE id = ?1",
            params![message, read],
        )?;
    }
    if let Some(starred) = change.starred {
        tx.execute(
            "UPDATE messages SET is_starred = ?2 WHERE id = ?1",
            params![message, starred],
        )?;
    }
    touched.insert(thread);
    Ok(())
}

fn delete(
    tx: &Transaction,
    account: i64,
    ids: &[RemoteId],
    touched: &mut BTreeSet<i64>,
) -> Result<usize, StoreError> {
    let mut deleted = 0;
    for id in ids {
        if let Some((message, thread)) = message_row(tx, account, id)? {
            tx.execute("DELETE FROM messages WHERE id = ?1", params![message])?;
            touched.insert(thread);
            deleted += 1;
        }
    }
    Ok(deleted)
}

fn store_body(
    tx: &Transaction,
    account: i64,
    remote: &str,
    body: &Body,
    now: i64,
    touched: &mut BTreeSet<i64>,
) -> Result<(), StoreError> {
    let Some((message, thread)) = message_row(tx, account, remote)? else {
        return Ok(());
    };
    let links = (!body.links.is_empty()).then(|| json(&body.links));
    let vouched = (!body.vouched_domains.is_empty()).then(|| json(&body.vouched_domains));
    tx.execute(
        "UPDATE messages SET body_text = ?2, body_html = ?3, body_fetched_at = ?4, links = ?5,
                             vouched_domains = ?6
         WHERE id = ?1",
        params![message, body.text, body.html, now, links, vouched],
    )?;
    touched.insert(thread);
    // A list the provider could not give says nothing: the stored one stays.
    if body.attachments_unknown {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM attachments WHERE message_id = ?1",
        params![message],
    )?;
    for a in &body.attachments {
        tx.execute(
            "INSERT INTO attachments (message_id, remote_part_id, filename, mime_type, size,
                                      plain_file, content_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message,
                a.part_id,
                a.filename,
                a.mime_type,
                a.size,
                a.plain_file,
                a.content_id
            ],
        )?;
    }
    Ok(())
}

fn mark_searched(
    tx: &Transaction,
    account: i64,
    ids: &[RemoteId],
    now: i64,
) -> Result<(), StoreError> {
    for id in ids {
        tx.execute(
            "UPDATE messages SET searched_at = ?3 WHERE account_id = ?1 AND remote_id = ?2 AND origin = 'search'",
            params![account, id, now],
        )?;
    }
    Ok(())
}

/// Delete the messages `condition` selects, noting their threads first.
fn delete_where(
    tx: &Transaction,
    condition: &str,
    args: &[&dyn ToSql],
    touched: &mut BTreeSet<i64>,
) -> Result<usize, StoreError> {
    let mut stmt = tx.prepare(&format!(
        "SELECT DISTINCT thread_id FROM messages WHERE {condition}"
    ))?;
    for thread in stmt.query_map(args, |r| r.get::<_, i64>(0))? {
        touched.insert(thread?);
    }
    Ok(tx.execute(&format!("DELETE FROM messages WHERE {condition}"), args)?)
}

fn sweep_unseen(
    tx: &Transaction,
    account: i64,
    sweep: Sweep,
    touched: &mut BTreeSet<i64>,
) -> Result<usize, StoreError> {
    let start = sweep.window_start_ms.unwrap_or(i64::MIN);
    delete_where(
        tx,
        &format!(
            "account_id = ?1 AND origin = 'sync' AND (synced_at IS NULL OR synced_at < ?2)
             AND internal_date >= ?3 AND {NOT_SNOOZED}"
        ),
        &[&account, &sweep.started_at, &start],
        touched,
    )
}

fn prune_old(
    tx: &Transaction,
    account: i64,
    prune: Prune,
    touched: &mut BTreeSet<i64>,
) -> Result<usize, StoreError> {
    let mut pruned = 0;
    if let Some(start) = prune.window_start_ms {
        pruned += delete_where(
            tx,
            &format!(
                "account_id = ?1 AND origin = 'sync' AND internal_date < ?2 AND is_starred = 0
                 AND NOT EXISTS (SELECT 1 FROM messages k WHERE k.thread_id = messages.thread_id
                                 AND k.internal_date >= ?2) AND {NOT_SNOOZED}"
            ),
            &[&account, &start],
            touched,
        )?;
    }
    let start = prune.window_start_ms.unwrap_or(i64::MIN);
    pruned += delete_where(
        tx,
        &format!(
            "account_id = ?1 AND origin = 'search' AND COALESCE(searched_at, 0) < ?2 AND is_starred = 0
             AND NOT EXISTS (SELECT 1 FROM messages k WHERE k.thread_id = messages.thread_id
                             AND k.internal_date >= ?3) AND {NOT_SNOOZED}"
        ),
        &[&account, &prune.search_before, &start],
        touched,
    )?;
    Ok(pruned)
}

/// `UPDATE accounts SET <column> = ?` for Rail's own column names only.
fn set_column(
    tx: &Transaction,
    account: i64,
    column: &str,
    value: &dyn ToSql,
) -> Result<(), StoreError> {
    tx.execute(
        &format!("UPDATE accounts SET {column} = ?2 WHERE id = ?1"),
        params![account, value],
    )?;
    Ok(())
}

fn set_resume(
    tx: &Transaction,
    account: i64,
    resume: Option<&ResumeToken>,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE accounts SET backfill_page_token = ?2, backfill_list_after_ms = ?3,
                backfill_list_before_ms = ?4, backfill_token_at = ?5 WHERE id = ?1",
        params![
            account,
            resume.map(|r| r.token.0.as_str()),
            resume.and_then(|r| r.listing.after_ms),
            resume.and_then(|r| r.listing.before_ms),
            resume.map(|r| r.issued_at),
        ],
    )?;
    Ok(())
}

fn update_state(tx: &Transaction, account: i64, state: &StateUpdate) -> Result<(), StoreError> {
    if let Some(v) = &state.cursor {
        set_column(tx, account, "sync_cursor", v)?;
    }
    if let Some(v) = &state.backfill_before {
        set_column(tx, account, "backfill_before", v)?;
    }
    if let Some(v) = &state.backfill_cursor {
        set_column(tx, account, "backfill_cursor", v)?;
    }
    if let Some(v) = &state.backfill_window {
        set_column(tx, account, "backfill_window", v)?;
    }
    if let Some(v) = &state.backfill_started_at {
        set_column(tx, account, "backfill_started_at", v)?;
    }
    if let Some(resume) = &state.backfill_resume {
        set_resume(tx, account, resume.as_ref())?;
    }
    if let Some(v) = &state.phase {
        set_column(tx, account, "sync_phase", v)?;
    }
    if let Some(v) = &state.last_sync_at {
        set_column(tx, account, "last_sync_at", v)?;
    }
    if let Some(error) = &state.last_error {
        set_column(
            tx,
            account,
            "last_error_code",
            &error.as_ref().map(|(code, _)| code.as_str()),
        )?;
        set_column(
            tx,
            account,
            "last_error_at",
            &error.as_ref().map(|(_, at)| *at),
        )?;
    }
    if let Some(v) = state.needs_auth {
        set_column(tx, account, "needs_auth", &v)?;
    }
    Ok(())
}
