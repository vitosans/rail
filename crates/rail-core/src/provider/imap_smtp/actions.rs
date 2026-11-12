//! Mail actions for generic IMAP accounts.
//!
//! Applies `Change` (Read, Star, Archive, Trash, Move, Unsnooze) to messages
//! selected by the single-folder scope rule (`actions::scope::touched`).
//! Flag changes execute via `UID STORE`. Moves use the IMAP `MOVE` extension
//! when available, falling back to atomic `UID COPY` + `UID STORE +FLAGS (\Deleted)` + `EXPUNGE`.

use std::collections::HashMap;

use async_imap::Session;

use super::client::{ImapCapabilities, ImapStream, classify_imap_error};
use super::folders::DiscoveredFolders;
use super::identity::ImapMessageId;
use crate::actions::scope::touched;
use crate::provider::{Change, FolderKind, ProviderError, ThreadTarget};

/// Executes a mail action on threads belonging to an IMAP account.
pub async fn apply_change(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    threads: &[ThreadTarget],
    change: &Change,
) -> Result<(), ProviderError> {
    if matches!(change, Change::Labels { .. }) {
        return Err(labels_unsupported());
    }

    let mut by_mailbox: HashMap<String, Vec<u32>> = HashMap::new();
    for thread in threads {
        for idx in touched(change, &thread.messages) {
            let msg = &thread.messages[idx];
            if let Some(parsed) = ImapMessageId::parse(msg.id.as_str()) {
                by_mailbox
                    .entry(parsed.mailbox)
                    .or_default()
                    .push(parsed.uid);
            }
        }
    }

    if by_mailbox.is_empty() {
        return Ok(());
    }

    for (mailbox, mut uids) in by_mailbox {
        uids.sort_unstable();
        uids.dedup();
        apply_change_to_mailbox(session, caps, folders, &mailbox, &uids, change).await?;
    }

    Ok(())
}

fn labels_unsupported() -> ProviderError {
    ProviderError::permanent(
        "IMAP accounts do not support arbitrary label assignment; use folders instead",
    )
}

async fn apply_change_to_mailbox(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    uids: &[u32],
    change: &Change,
) -> Result<(), ProviderError> {
    if uids.is_empty() {
        return Ok(());
    }

    session
        .select(source_mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let uids_str = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    apply_to_uids(session, caps, folders, source_mailbox, &uids_str, change).await
}

async fn apply_to_uids(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    uids_str: &str,
    change: &Change,
) -> Result<(), ProviderError> {
    match change {
        Change::Read { on } => store_seen(session, uids_str, *on).await,
        Change::Star { on } => store_starred(session, uids_str, *on).await,
        Change::Move { to } => move_uids(session, caps, uids_str, to.as_str()).await,
        Change::Archive => archive(session, caps, folders, source_mailbox, uids_str).await,
        Change::Trash => trash(session, caps, folders, source_mailbox, uids_str).await,
        Change::Unsnooze { .. } => unsnooze(session, caps, source_mailbox, uids_str).await,
        Change::Labels { .. } => Err(labels_unsupported()),
    }
}

async fn store_seen(
    session: &mut Session<ImapStream>,
    uids_str: &str,
    on: bool,
) -> Result<(), ProviderError> {
    let query = if on {
        "+FLAGS (\\Seen)"
    } else {
        "-FLAGS (\\Seen)"
    };
    store_flags(session, uids_str, query).await
}

async fn store_starred(
    session: &mut Session<ImapStream>,
    uids_str: &str,
    on: bool,
) -> Result<(), ProviderError> {
    let query = if on {
        "+FLAGS (\\Flagged)"
    } else {
        "-FLAGS (\\Flagged)"
    };
    store_flags(session, uids_str, query).await
}

async fn archive(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    uids_str: &str,
) -> Result<(), ProviderError> {
    if let Some(folder) = folders.find_by_kind(FolderKind::Archive)
        && folder.name != source_mailbox
    {
        move_uids(session, caps, uids_str, &folder.name).await?;
    }
    Ok(())
}

async fn trash(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    uids_str: &str,
) -> Result<(), ProviderError> {
    match folders.trash_mailbox_name() {
        Some(trash_mailbox) if trash_mailbox != source_mailbox => {
            move_uids(session, caps, uids_str, trash_mailbox).await
        }
        Some(_) => Ok(()),
        None => delete_and_expunge(session, uids_str).await,
    }
}

async fn unsnooze(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    source_mailbox: &str,
    uids_str: &str,
) -> Result<(), ProviderError> {
    if source_mailbox != "INBOX" {
        move_uids(session, caps, uids_str, "INBOX").await?;
    }
    Ok(())
}

async fn store_flags(
    session: &mut Session<ImapStream>,
    uids_str: &str,
    query: &str,
) -> Result<(), ProviderError> {
    use futures_util::StreamExt;
    let stream = session
        .uid_store(uids_str, query)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    tokio::pin!(stream);
    while let Some(res) = stream.next().await {
        let _ = res.map_err(|e| classify_imap_error(&e))?;
    }
    Ok(())
}

async fn move_uids(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    uids_str: &str,
    destination: &str,
) -> Result<(), ProviderError> {
    if caps.move_support {
        match session.uid_mv(uids_str, destination).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                tracing::warn!(
                    destination = %destination,
                    error = %e,
                    "UID MOVE failed; falling back to copy-then-delete"
                );
            }
        }
    }

    session
        .uid_copy(uids_str, destination)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    delete_and_expunge(session, uids_str).await
}

async fn delete_and_expunge(
    session: &mut Session<ImapStream>,
    uids_str: &str,
) -> Result<(), ProviderError> {
    use futures_util::StreamExt;
    {
        let store_stream = session
            .uid_store(uids_str, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| classify_imap_error(&e))?;
        tokio::pin!(store_stream);
        while let Some(res) = store_stream.next().await {
            let _ = res.map_err(|e| classify_imap_error(&e))?;
        }
    }

    {
        let expunge_stream = session
            .expunge()
            .await
            .map_err(|e| classify_imap_error(&e))?;
        tokio::pin!(expunge_stream);
        while let Some(res) = expunge_stream.next().await {
            let _ = res.map_err(|e| classify_imap_error(&e))?;
        }
    }

    Ok(())
}
