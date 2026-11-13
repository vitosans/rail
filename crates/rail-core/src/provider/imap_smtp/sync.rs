//! UID range backfill, delta sync loops, and IDLE/bounded polling for IMAP.

use std::time::Duration;

use async_imap::Session;
use async_imap::types::Flag;
use futures_util::StreamExt;

use super::client::{ImapStream, classify_imap_error};
use super::identity::ImapMessageId;
use super::read::fetch_envelopes;
use crate::provider::{ChangeSet, ProviderError, Relabel, RemoteId, SyncCursor};

/// Parsed representation of an IMAP sync cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapSyncCursor {
    pub mailbox: String,
    pub uidvalidity: u32,
    pub last_uid: u32,
}

impl ImapSyncCursor {
    pub fn new(mailbox: impl Into<String>, uidvalidity: u32, last_uid: u32) -> Self {
        Self {
            mailbox: mailbox.into(),
            uidvalidity,
            last_uid,
        }
    }

    /// Formats as standard `SyncCursor`: `{mailbox}:{uidvalidity}:{last_uid}`.
    pub fn to_sync_cursor(&self) -> SyncCursor {
        SyncCursor(format!(
            "{}:{}:{}",
            self.mailbox, self.uidvalidity, self.last_uid
        ))
    }

    /// Parses a `SyncCursor` into its mailbox, uidvalidity, and last seen UID.
    pub fn parse(cursor: &SyncCursor) -> Option<Self> {
        let mut parts = cursor.0.rsplitn(3, ':');
        let last_uid = parts.next()?.parse().ok()?;
        let uidvalidity = parts.next()?.parse().ok()?;
        let mailbox = parts.next()?;
        if mailbox.is_empty() {
            return None;
        }
        Some(Self {
            mailbox: mailbox.to_string(),
            uidvalidity,
            last_uid,
        })
    }
}

/// Discovers changes in a mailbox since the provided sync cursor.
pub async fn sync_changes(
    session: &mut Session<ImapStream>,
    cursor: &SyncCursor,
    page_size: usize,
) -> Result<ChangeSet, ProviderError> {
    let parsed = ImapSyncCursor::parse(cursor).ok_or(ProviderError::CursorExpired)?;
    let examine_status = session
        .examine(&parsed.mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let current_uidvalidity = examine_status.uid_validity.unwrap_or(0);
    if current_uidvalidity != parsed.uidvalidity {
        return Err(ProviderError::CursorExpired);
    }

    let (added, next_uid, more) = fetch_new_messages(session, &parsed, page_size).await?;
    let relabeled = fetch_flag_updates(session, &parsed).await?;

    let next_cursor = ImapSyncCursor::new(&parsed.mailbox, parsed.uidvalidity, next_uid);

    Ok(ChangeSet {
        added: added.envelopes,
        deleted: Vec::new(),
        relabeled,
        skipped: added.skipped,
        cursor: next_cursor.to_sync_cursor(),
        more,
    })
}

async fn fetch_new_messages(
    session: &mut Session<ImapStream>,
    parsed: &ImapSyncCursor,
    page_size: usize,
) -> Result<(crate::provider::EnvelopePage, u32, bool), ProviderError> {
    let query = if parsed.last_uid > 0 {
        format!("UID {}:*", parsed.last_uid.saturating_add(1))
    } else {
        "ALL".to_string()
    };

    let uids_set = session
        .uid_search(&query)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut new_uids: Vec<u32> = uids_set
        .into_iter()
        .filter(|&uid| uid > parsed.last_uid)
        .collect();
    new_uids.sort_unstable();

    if new_uids.is_empty() {
        return Ok((
            crate::provider::EnvelopePage::default(),
            parsed.last_uid,
            false,
        ));
    }

    let (page_uids, more) = if new_uids.len() > page_size {
        (&new_uids[..page_size], true)
    } else {
        (&new_uids[..], false)
    };

    let ids: Vec<RemoteId> = page_uids
        .iter()
        .map(|&uid| ImapMessageId::new(&parsed.mailbox, parsed.uidvalidity, uid).to_remote_id())
        .collect();

    let envelopes = fetch_envelopes(session, &ids).await?;
    // Advance only to the highest UID whose envelope actually arrived. A fetch that lost a
    // record must leave its UID for the next pass.
    let highest_received = envelopes
        .envelopes
        .iter()
        .filter_map(|e| ImapMessageId::parse(&e.remote_id).map(|id| id.uid))
        .max();
    let next_uid =
        highest_received.unwrap_or_else(|| *page_uids.last().unwrap_or(&parsed.last_uid));
    Ok((envelopes, next_uid, more))
}

async fn fetch_flag_updates(
    session: &mut Session<ImapStream>,
    parsed: &ImapSyncCursor,
) -> Result<Vec<Relabel>, ProviderError> {
    if parsed.last_uid == 0 {
        return Ok(Vec::new());
    }

    let start_uid = parsed.last_uid.saturating_sub(500).max(1);
    let query = format!("{start_uid}:{}", parsed.last_uid);

    let mut relabels = Vec::new();
    let mut stream = session
        .uid_fetch(&query, "(FLAGS)")
        .await
        .map_err(|e| classify_imap_error(&e))?;

    while let Some(fetch_result) = stream.next().await {
        let fetch = fetch_result.map_err(|e| classify_imap_error(&e))?;
        let Some(uid) = fetch.uid else { continue };
        if uid > parsed.last_uid {
            continue;
        }

        let mut is_read = false;
        let mut is_starred = false;
        for flag in fetch.flags() {
            match flag {
                Flag::Seen => is_read = true,
                Flag::Flagged => is_starred = true,
                _ => {}
            }
        }

        let remote_id = ImapMessageId::new(&parsed.mailbox, parsed.uidvalidity, uid).to_remote_id();
        relabels.push(Relabel {
            remote_id,
            read: Some(is_read),
            starred: Some(is_starred),
            ..Relabel::default()
        });
    }

    Ok(relabels)
}

/// Waits for mailbox updates using IMAP IDLE when supported, or sleeps for `poll_interval`.
pub async fn idle_wait(
    session: Session<ImapStream>,
    idle_supported: bool,
    poll_interval: Duration,
) -> Result<Session<ImapStream>, ProviderError> {
    if !idle_supported {
        tokio::time::sleep(poll_interval).await;
        return Ok(session);
    }

    let mut idle = session.idle();
    let (fut, _stop) = idle.wait_with_timeout(poll_interval);
    let _ = fut.await;
    let session = idle.done().await.map_err(|e| classify_imap_error(&e))?;
    Ok(session)
}
