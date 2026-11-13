//! Mail actions for generic IMAP accounts.
//!
//! Applies `Change` (Read, Star, Archive, Trash, Move, Unsnooze) to messages
//! selected by the single-folder scope rule (`actions::scope::touched`).
//! Flag changes execute via `UID STORE`. Moves use the IMAP `MOVE` extension when available;
//! without it they use `UID COPY` followed by `UID STORE +FLAGS (\Deleted)` and `UID EXPUNGE`,
//! which needs UIDPLUS. A plain EXPUNGE could remove another client's deleted mail.

use std::collections::HashMap;

use async_imap::Session;
use async_imap::imap_proto::rfc4315::UidSetMember;
use async_imap::imap_proto::{Response, ResponseCode, Status};

use super::client::{ImapCapabilities, ImapStream, classify_imap_error};
use super::folders::DiscoveredFolders;
use super::identity::ImapMessageId;
use crate::actions::scope::touched;
use crate::provider::{Change, FolderKind, ProviderError, RemoteId, ThreadTarget};

type MoveMap = Vec<(RemoteId, RemoteId)>;

/// Executes a mail action on threads belonging to an IMAP account, returning old-to-new
/// provider ids for any messages that moved.
pub async fn apply_change(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    threads: &[ThreadTarget],
    change: &Change,
) -> Result<MoveMap, ProviderError> {
    if matches!(change, Change::Labels { .. }) {
        return Err(labels_unsupported());
    }

    let mut by_mailbox: HashMap<(String, u32), Vec<u32>> = HashMap::new();
    for thread in threads {
        for idx in touched(change, &thread.messages) {
            let msg = &thread.messages[idx];
            if let Some(parsed) = ImapMessageId::parse(msg.id.as_str()) {
                by_mailbox
                    .entry((parsed.mailbox, parsed.uidvalidity))
                    .or_default()
                    .push(parsed.uid);
            }
        }
    }

    let mut moves = Vec::new();
    for ((mailbox, uidvalidity), mut uids) in by_mailbox {
        uids.sort_unstable();
        uids.dedup();
        moves.extend(
            apply_change_to_mailbox(session, caps, folders, &mailbox, uidvalidity, &uids, change)
                .await?,
        );
    }

    Ok(moves)
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
    uidvalidity: u32,
    uids: &[u32],
    change: &Change,
) -> Result<MoveMap, ProviderError> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }

    let status = session
        .select(source_mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    if status.uid_validity.unwrap_or(0) != uidvalidity {
        return Err(ProviderError::CursorExpired);
    }

    let uids_str = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    apply_to_uids(
        session,
        caps,
        folders,
        source_mailbox,
        uidvalidity,
        &uids_str,
        change,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn apply_to_uids(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    source_uidvalidity: u32,
    uids_str: &str,
    change: &Change,
) -> Result<MoveMap, ProviderError> {
    match change {
        Change::Read { on } => store_seen(session, uids_str, *on).await.map(|_| Vec::new()),
        Change::Star { on } => store_starred(session, uids_str, *on)
            .await
            .map(|_| Vec::new()),
        Change::Move { to } => {
            move_uids(
                session,
                caps,
                source_mailbox,
                source_uidvalidity,
                uids_str,
                to.as_str(),
            )
            .await
        }
        Change::Archive => {
            archive(
                session,
                caps,
                folders,
                source_mailbox,
                source_uidvalidity,
                uids_str,
            )
            .await
        }
        Change::Trash => {
            trash(
                session,
                caps,
                folders,
                source_mailbox,
                source_uidvalidity,
                uids_str,
            )
            .await
        }
        Change::Unsnooze { .. } => {
            unsnooze(session, caps, source_mailbox, source_uidvalidity, uids_str).await
        }
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
    source_uidvalidity: u32,
    uids_str: &str,
) -> Result<MoveMap, ProviderError> {
    match folders.find_by_kind(FolderKind::Archive) {
        Some(folder) if folder.name != source_mailbox => {
            move_uids(
                session,
                caps,
                source_mailbox,
                source_uidvalidity,
                uids_str,
                &folder.name,
            )
            .await
        }
        _ => Err(ProviderError::permanent(
            "This mailbox has no Archive folder; press m to move.",
        )),
    }
}

async fn trash(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    folders: &DiscoveredFolders,
    source_mailbox: &str,
    source_uidvalidity: u32,
    uids_str: &str,
) -> Result<MoveMap, ProviderError> {
    match folders.trash_mailbox_name() {
        Some(trash_mailbox) if trash_mailbox != source_mailbox => {
            move_uids(
                session,
                caps,
                source_mailbox,
                source_uidvalidity,
                uids_str,
                trash_mailbox,
            )
            .await
        }
        Some(_) => Ok(Vec::new()),
        None => Err(ProviderError::permanent(
            "This mailbox has no Trash folder, so Rail will not delete the mail permanently.",
        )),
    }
}

async fn unsnooze(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    source_mailbox: &str,
    source_uidvalidity: u32,
    uids_str: &str,
) -> Result<MoveMap, ProviderError> {
    if source_mailbox != "INBOX" {
        move_uids(
            session,
            caps,
            source_mailbox,
            source_uidvalidity,
            uids_str,
            "INBOX",
        )
        .await
    } else {
        Ok(Vec::new())
    }
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
    source_mailbox: &str,
    source_uidvalidity: u32,
    uids_str: &str,
    destination: &str,
) -> Result<MoveMap, ProviderError> {
    if caps.move_support {
        let copyuid =
            command_copyuid(session, &format!("UID MOVE {uids_str} {destination}")).await?;
        return Ok(moves_from_copyuid(
            source_mailbox,
            source_uidvalidity,
            destination,
            copyuid,
        ));
    }

    if !caps.uidplus_support {
        return Err(ProviderError::permanent(
            "this IMAP server supports neither MOVE nor UIDPLUS, so Rail cannot move mail safely",
        ));
    }

    let copyuid = command_copyuid(session, &format!("UID COPY {uids_str} {destination}")).await?;
    let moves = moves_from_copyuid(source_mailbox, source_uidvalidity, destination, copyuid);
    delete_and_expunge(session, uids_str).await?;
    Ok(moves)
}

fn moves_from_copyuid(
    source_mailbox: &str,
    source_uidvalidity: u32,
    destination: &str,
    copyuid: Option<(u32, Vec<(u32, u32)>)>,
) -> MoveMap {
    let Some((destination_uidvalidity, pairs)) = copyuid else {
        return Vec::new();
    };
    pairs
        .into_iter()
        .map(|(old, new)| {
            (
                ImapMessageId::new(source_mailbox, source_uidvalidity, old).to_remote_id(),
                ImapMessageId::new(destination, destination_uidvalidity, new).to_remote_id(),
            )
        })
        .collect()
}

/// Runs a command and reads its tagged answer, keeping a COPYUID code if the server sends one.
async fn command_copyuid(
    session: &mut Session<ImapStream>,
    command: &str,
) -> Result<Option<(u32, Vec<(u32, u32)>)>, ProviderError> {
    let id = session
        .run_command(command)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    loop {
        let Some(response) = session.read_response().await.map_err(|e| {
            ProviderError::transient("IMAP network error").with_detail(e.to_string())
        })?
        else {
            return Err(ProviderError::transient("IMAP connection lost"));
        };
        let parsed = response.parsed();
        match parsed {
            Response::Data { outcome, .. } => {
                if let Some(found) = copyuid_of(outcome.code.as_ref()) {
                    return Ok(Some(found));
                }
            }
            Response::Done {
                tag,
                status,
                outcome,
            } if tag == &id => {
                if *status != Status::Ok {
                    let text = outcome
                        .information
                        .as_ref()
                        .map_or_else(String::new, ToString::to_string);
                    return Err(
                        ProviderError::permanent("the IMAP server refused the operation")
                            .with_detail(text),
                    );
                }
                return Ok(copyuid_of(outcome.code.as_ref()));
            }
            _ => {}
        }
    }
}

fn copyuid_of(code: Option<&ResponseCode<'_>>) -> Option<(u32, Vec<(u32, u32)>)> {
    let ResponseCode::CopyUid(uidvalidity, source, destination) = code? else {
        return None;
    };
    let source = expand_uid_set(source);
    let destination = expand_uid_set(destination);
    if source.len() != destination.len() {
        return None;
    }
    Some((*uidvalidity, source.into_iter().zip(destination).collect()))
}

fn expand_uid_set(set: &[UidSetMember]) -> Vec<u32> {
    let mut uids = Vec::new();
    for member in set {
        match member {
            UidSetMember::Uid(uid) => uids.push(*uid),
            UidSetMember::UidRange(range) => uids.extend(range.clone()),
        }
    }
    uids
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
            .uid_expunge(uids_str)
            .await
            .map_err(|e| classify_imap_error(&e))?;
        tokio::pin!(expunge_stream);
        while let Some(res) = expunge_stream.next().await {
            let _ = res.map_err(|e| classify_imap_error(&e))?;
        }
    }

    Ok(())
}
