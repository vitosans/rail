//! Mailbox discovery, RFC 6154 special-use mapping, and user overrides.

use async_imap::types::{Name, NameAttribute};
use futures_util::TryStreamExt;

use super::client::{ImapStream, classify_imap_error};
use crate::provider::{FolderKind, ProviderError, RemoteFolder, RemoteId};

/// Mailboxes discovered from an IMAP server, mapped to Rail semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveredFolders {
    pub folders: Vec<RemoteFolder>,
}

impl DiscoveredFolders {
    pub fn new(folders: Vec<RemoteFolder>) -> Self {
        Self { folders }
    }

    pub fn find_by_kind(&self, kind: FolderKind) -> Option<&RemoteFolder> {
        self.folders.iter().find(|f| f.kind == kind)
    }

    pub fn find_by_id(&self, id: &str) -> Option<&RemoteFolder> {
        self.folders.iter().find(|f| f.remote_id.as_str() == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&RemoteFolder> {
        self.folders
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }

    pub fn sent_mailbox_name(&self) -> &str {
        self.find_by_kind(FolderKind::Sent)
            .map(|f| f.name.as_str())
            .unwrap_or("Sent")
    }

    pub fn drafts_mailbox_name(&self) -> &str {
        self.find_by_kind(FolderKind::Drafts)
            .map(|f| f.name.as_str())
            .unwrap_or("Drafts")
    }

    pub fn trash_mailbox_name(&self) -> Option<&str> {
        self.find_by_kind(FolderKind::Trash)
            .map(|f| f.name.as_str())
    }

    pub fn archive_mailbox_name(&self) -> Option<&str> {
        self.find_by_kind(FolderKind::Archive)
            .map(|f| f.name.as_str())
    }
}

/// Discover all mailboxes from an active IMAP session.
pub async fn discover_folders(
    session: &mut async_imap::Session<ImapStream>,
    sent_folder_override: Option<&str>,
) -> Result<DiscoveredFolders, ProviderError> {
    let mut stream = session
        .list(Some(""), Some("*"))
        .await
        .map_err(|e| classify_imap_error(&e))?;

    let mut folders = Vec::new();
    while let Some(name) = stream
        .try_next()
        .await
        .map_err(|e| classify_imap_error(&e))?
    {
        if is_selectable(&name) {
            folders.push(map_imap_mailbox(&name, sent_folder_override));
        }
    }

    Ok(DiscoveredFolders::new(folders))
}

/// Check if a mailbox is selectable (does not have \Noselect).
pub fn is_selectable(name: &Name) -> bool {
    !name
        .attributes()
        .iter()
        .any(|attr| matches!(attr, NameAttribute::NoSelect))
}

/// Map an IMAP mailbox `Name` to Rail's `RemoteFolder`.
pub fn map_imap_mailbox(name: &Name, sent_folder_override: Option<&str>) -> RemoteFolder {
    let mailbox_name = name.name();
    let kind = resolve_folder_kind(mailbox_name, name.attributes(), sent_folder_override);
    let parent = resolve_parent(mailbox_name, name.delimiter());

    RemoteFolder {
        remote_id: RemoteId::from(mailbox_name),
        name: mailbox_name.to_string(),
        kind,
        parent,
    }
}

/// Resolve `FolderKind` based on overrides, RFC 6154 attributes, and name heuristics.
pub fn resolve_folder_kind(
    name: &str,
    attributes: &[NameAttribute<'_>],
    sent_folder_override: Option<&str>,
) -> FolderKind {
    // 1. User sent folder override
    if let Some(sent_override) = sent_folder_override
        && name.eq_ignore_ascii_case(sent_override)
    {
        return FolderKind::Sent;
    }

    // 2. RFC 6154 Special-Use attributes
    if let Some(kind) = map_special_use_attributes(attributes) {
        return kind;
    }

    // 3. Well-known INBOX
    if name.eq_ignore_ascii_case("INBOX") {
        return FolderKind::Inbox;
    }

    // 4. Fallback name heuristics
    map_fallback_name(name)
}

fn map_special_use_attributes(attributes: &[NameAttribute<'_>]) -> Option<FolderKind> {
    for attr in attributes {
        match attr {
            NameAttribute::Sent => return Some(FolderKind::Sent),
            NameAttribute::Drafts => return Some(FolderKind::Drafts),
            NameAttribute::Trash => return Some(FolderKind::Trash),
            NameAttribute::Archive => return Some(FolderKind::Archive),
            NameAttribute::Junk => return Some(FolderKind::Spam),
            NameAttribute::All => return Some(FolderKind::Archive),
            _ => {}
        }
    }
    None
}

pub fn map_fallback_name(name: &str) -> FolderKind {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "sent" | "sent mail" | "sent messages" | "sent items" => FolderKind::Sent,
        "drafts" | "draft" => FolderKind::Drafts,
        "trash" | "deleted" | "deleted messages" | "deleted items" | "bin" => FolderKind::Trash,
        "archive" | "archives" | "all mail" => FolderKind::Archive,
        "junk" | "junk mail" | "junk email" | "spam" => FolderKind::Spam,
        _ => FolderKind::User,
    }
}

pub fn resolve_parent(name: &str, delimiter: Option<&str>) -> Option<RemoteId> {
    let delim = delimiter?;
    if delim.is_empty() {
        return None;
    }
    let parts: Vec<&str> = name.rsplitn(2, delim).collect();
    if parts.len() == 2 && !parts[1].is_empty() {
        Some(RemoteId::from(parts[1]))
    } else {
        None
    }
}
