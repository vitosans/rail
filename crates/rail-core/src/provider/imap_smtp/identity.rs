//! Deterministic message remote IDs, RFC 5322 header threading, and UIDVALIDITY tracking.

use sha2::{Digest, Sha256};

use crate::provider::RemoteId;

/// Parsed elements of an IMAP message remote ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImapMessageId {
    pub mailbox: String,
    pub uidvalidity: u32,
    pub uid: u32,
}

impl ImapMessageId {
    pub fn new(mailbox: impl Into<String>, uidvalidity: u32, uid: u32) -> Self {
        Self {
            mailbox: mailbox.into(),
            uidvalidity,
            uid,
        }
    }

    /// Format as stable `RemoteId`: `{mailbox}:{uidvalidity}:{uid}`.
    pub fn to_remote_id(&self) -> RemoteId {
        format!("{}:{}:{}", self.mailbox, self.uidvalidity, self.uid)
    }

    /// Parse a `RemoteId` into its IMAP mailbox, uidvalidity, and uid.
    pub fn parse(remote_id: &str) -> Option<Self> {
        let mut parts = remote_id.rsplitn(3, ':');
        let uid_str = parts.next()?;
        let uidvalidity_str = parts.next()?;
        let mailbox = parts.next()?;

        let uid = uid_str.parse().ok()?;
        let uidvalidity = uidvalidity_str.parse().ok()?;
        if mailbox.is_empty() {
            return None;
        }

        Some(Self {
            mailbox: mailbox.to_string(),
            uidvalidity,
            uid,
        })
    }
}

/// Outcome of checking a mailbox's UIDVALIDITY against a stored epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UidValidityCheck {
    /// Epoch is unchanged from previous sync.
    Unchanged,
    /// First time this mailbox's UIDVALIDITY is being recorded.
    Initial(u32),
    /// Server has reset UIDVALIDITY, requiring full re-identification by Message-ID.
    Reset { previous: u32, current: u32 },
}

/// Compare a previous recorded UIDVALIDITY with the current value reported by IMAP.
pub fn check_uidvalidity(previous: Option<u32>, current: u32) -> UidValidityCheck {
    match previous {
        None => UidValidityCheck::Initial(current),
        Some(prev) if prev == current => UidValidityCheck::Unchanged,
        Some(prev) => UidValidityCheck::Reset {
            previous: prev,
            current,
        },
    }
}

/// Clean a Message-ID header, removing enclosing angle brackets and whitespace.
pub fn clean_message_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix('<')
        .and_then(|r| r.strip_suffix('>'))
        .unwrap_or(trimmed)
        .trim();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// Compute a deterministic thread identifier from RFC 5322 header chains.
///
/// If `references` contains valid IDs, the first reference is considered the root.
/// Otherwise, if `in_reply_to` is present, it is considered the root.
/// Otherwise, the message's own `message_id` is the root.
pub fn compute_thread_id(
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
) -> RemoteId {
    let root = resolve_thread_root(message_id, in_reply_to, references);
    let mut hasher = Sha256::new();
    hasher.update(root.to_lowercase().as_bytes());
    let hash = hasher.finalize();
    let mut hex = String::with_capacity(7 + hash.len() * 2);
    hex.push_str("thread:");
    for b in hash {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

fn resolve_thread_root(
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
) -> String {
    for ref_str in references {
        if let Some(clean) = clean_message_id(ref_str) {
            return clean;
        }
    }

    if let Some(parent) = in_reply_to.and_then(clean_message_id) {
        return parent;
    }

    clean_message_id(message_id).unwrap_or_else(|| message_id.trim().to_string())
}
