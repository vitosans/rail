//! Two-phase mail submission: SMTP relay acceptance followed by IMAP Sent copy.
//!
//! When SMTP accepts a message, the send is considered permanently accepted. If the subsequent
//! append to the IMAP Sent mailbox fails (network blip, quota, etc.), the send remains accepted
//! and only the Sent copy is retried; SMTP is NEVER resubmitted.

use async_imap::Session;

use super::client::{ImapStream, classify_imap_error};
use super::drafts::find_draft;
use super::smtp::{SmtpTransport, send_smtp};
use crate::provider::gmail::mime::build;
use crate::provider::{OutgoingMail, ProviderError, RemoteId, SentReceipt};

/// Result of a two-phase send operation.
#[derive(Debug, PartialEq, Eq)]
pub enum TwoPhaseOutcome {
    /// Both SMTP relay submission and IMAP Sent-folder copy succeeded.
    Success(SentReceipt),
    /// SMTP relay submission succeeded, but IMAP Sent-folder copy failed.
    SentFilePending {
        receipt: SentReceipt,
        error: ProviderError,
    },
}

/// Executes a two-phase send: SMTP submission followed by IMAP Sent append.
pub async fn send_two_phase(
    transport: &SmtpTransport,
    session: Option<&mut Session<ImapStream>>,
    sent_mailbox: &str,
    mail: &OutgoingMail,
) -> Result<TwoPhaseOutcome, ProviderError> {
    // Phase 1: SMTP relay submission
    let receipt = send_smtp(transport, mail).await?;

    // Phase 2: IMAP Sent-folder append
    let Some(session) = session else {
        return Ok(TwoPhaseOutcome::Success(receipt));
    };

    match append_to_sent(session, sent_mailbox, mail).await {
        Ok(sent_id) => {
            let mut final_receipt = receipt;
            if sent_id.is_some() {
                final_receipt.message_id = sent_id;
            }
            Ok(TwoPhaseOutcome::Success(final_receipt))
        }
        Err(err) => {
            tracing::warn!(
                message_id = %mail.outgoing.message_id,
                error = %err,
                "SMTP relay accepted message, but IMAP Sent append failed"
            );
            Ok(TwoPhaseOutcome::SentFilePending {
                receipt,
                error: err,
            })
        }
    }
}

/// Appends the outgoing message bytes into the configured Sent mailbox with the `\Seen` flag.
pub async fn append_to_sent(
    session: &mut Session<ImapStream>,
    sent_mailbox: &str,
    mail: &OutgoingMail,
) -> Result<Option<RemoteId>, ProviderError> {
    let raw_bytes = build(&mail.outgoing)?;

    session
        .append(sent_mailbox, Some("(\\Seen)"), None, &raw_bytes)
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let found_id = find_draft(session, sent_mailbox, &mail.outgoing.message_id)
        .await
        .ok()
        .flatten();

    Ok(found_id)
}

/// Checks whether the provider's Sent mailbox already contains the message by its Message-ID.
pub async fn check_sent_already(
    session: &mut Session<ImapStream>,
    sent_mailbox: &str,
    message_id: &str,
) -> Result<bool, ProviderError> {
    match find_draft(session, sent_mailbox, message_id).await {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(e) => {
            tracing::warn!(
                message_id,
                error = %e,
                "failed to query Sent folder for existing message"
            );
            Ok(false)
        }
    }
}
