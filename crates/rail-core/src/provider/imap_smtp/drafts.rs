//! Server-side draft storage, retrieval, and deletion in the IMAP Drafts mailbox.

use async_imap::Session;
use futures_util::StreamExt;

use super::client::{ImapCapabilities, ImapStream, classify_imap_error};
use super::identity::ImapMessageId;
use crate::provider::gmail::mime::build;
use crate::provider::{DraftIds, OutgoingMail, ProviderError, RemoteId, SavedDraft};

/// Appends or updates an RFC 5322 draft message in the configured Drafts mailbox.
pub async fn save_draft(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    drafts_mailbox: &str,
    mail: &OutgoingMail,
) -> Result<SavedDraft, ProviderError> {
    if let Some(existing) = &mail.remote {
        let _ = delete_draft(session, caps, &existing.draft_id).await;
    }

    let raw_bytes = build(&mail.outgoing)?;

    session
        .append(drafts_mailbox, Some("(\\Draft \\Seen)"), None, &raw_bytes)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let found_id = find_draft(session, drafts_mailbox, &mail.outgoing.message_id).await?;
    let id_str =
        found_id.unwrap_or_else(|| format!("{drafts_mailbox}:0:{}", mail.outgoing.message_id));

    Ok(SavedDraft {
        ids: DraftIds {
            draft_id: id_str.clone(),
            message_id: id_str,
        },
        listed: Vec::new(),
    })
}

/// Deletes a draft message by remote ID from the server Drafts mailbox.
pub async fn delete_draft(
    session: &mut Session<ImapStream>,
    caps: &ImapCapabilities,
    draft_id: &str,
) -> Result<(), ProviderError> {
    let parsed = ImapMessageId::parse(draft_id).ok_or(ProviderError::NotFound)?;

    let status = session
        .select(&parsed.mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    if status.uid_validity.unwrap_or(0) != parsed.uidvalidity {
        return Err(ProviderError::NotFound);
    }
    if !caps.uidplus_support {
        return Err(ProviderError::permanent(
            "this IMAP server does not support UIDPLUS, so Rail cannot safely expunge one message",
        ));
    }

    let uids_str = parsed.uid.to_string();

    {
        let store_stream = session
            .uid_store(&uids_str, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| classify_imap_error(&e))?;
        tokio::pin!(store_stream);
        let mut found = false;
        while let Some(res) = store_stream.next().await {
            let _ = res.map_err(|e| classify_imap_error(&e))?;
            found = true;
        }
        if !found {
            return Err(ProviderError::NotFound);
        }
    }

    {
        let expunge_stream = session
            .uid_expunge(&uids_str)
            .await
            .map_err(|e| classify_imap_error(&e))?;
        tokio::pin!(expunge_stream);
        while let Some(res) = expunge_stream.next().await {
            let _ = res.map_err(|e| classify_imap_error(&e))?;
        }
    }

    Ok(())
}

/// Looks up an existing draft in the Drafts mailbox by RFC 5322 Message-ID header.
pub async fn find_draft(
    session: &mut Session<ImapStream>,
    drafts_mailbox: &str,
    message_id: &str,
) -> Result<Option<RemoteId>, ProviderError> {
    let examine_status = session
        .examine(drafts_mailbox)
        .await
        .map_err(|e| classify_imap_error(&e))?;
    let uidvalidity = examine_status.uid_validity.unwrap_or(0);

    let clean_msg_id = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .replace(['\\', '"', '\r', '\n'], "");

    let criteria = format!("HEADER Message-ID \"{clean_msg_id}\"");
    let uids_set = session
        .uid_search(&criteria)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut uids: Vec<u32> = uids_set.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));

    if let Some(&uid) = uids.first() {
        let remote = ImapMessageId::new(drafts_mailbox, uidvalidity, uid).to_remote_id();
        Ok(Some(remote))
    } else {
        Ok(None)
    }
}
