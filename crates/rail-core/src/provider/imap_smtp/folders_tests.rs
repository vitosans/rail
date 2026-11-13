use async_imap::types::NameAttribute;

use super::folders::{DiscoveredFolders, resolve_folder_kind, resolve_parent};
use crate::provider::{FolderKind, RemoteFolder, RemoteId};

#[test]
fn test_special_use_attributes() {
    assert_eq!(
        resolve_folder_kind("ArbitraryName", &[NameAttribute::Sent], None),
        FolderKind::Sent
    );
    assert_eq!(
        resolve_folder_kind("ArbitraryName", &[NameAttribute::Drafts], None),
        FolderKind::Drafts
    );
    assert_eq!(
        resolve_folder_kind("ArbitraryName", &[NameAttribute::Trash], None),
        FolderKind::Trash
    );
    assert_eq!(
        resolve_folder_kind("ArbitraryName", &[NameAttribute::Archive], None),
        FolderKind::Archive
    );
    assert_eq!(
        resolve_folder_kind("ArbitraryName", &[NameAttribute::Junk], None),
        FolderKind::Spam
    );
}

#[test]
fn test_user_sent_override() {
    // Override takes precedence over RFC 6154 attribute
    assert_eq!(
        resolve_folder_kind("MySent", &[NameAttribute::Archive], Some("MySent")),
        FolderKind::Sent
    );
    // Case-insensitive override match
    assert_eq!(
        resolve_folder_kind("sent mail", &[], Some("Sent Mail")),
        FolderKind::Sent
    );
}

#[test]
fn test_fallback_name_heuristics() {
    assert_eq!(resolve_folder_kind("INBOX", &[], None), FolderKind::Inbox);
    assert_eq!(resolve_folder_kind("inbox", &[], None), FolderKind::Inbox);
    assert_eq!(
        resolve_folder_kind("Sent Items", &[], None),
        FolderKind::Sent
    );
    assert_eq!(
        resolve_folder_kind("sent mail", &[], None),
        FolderKind::Sent
    );
    assert_eq!(resolve_folder_kind("Drafts", &[], None), FolderKind::Drafts);
    assert_eq!(
        resolve_folder_kind("Deleted Items", &[], None),
        FolderKind::Trash
    );
    assert_eq!(resolve_folder_kind("Trash", &[], None), FolderKind::Trash);
    assert_eq!(resolve_folder_kind("Bin", &[], None), FolderKind::Trash);
    assert_eq!(
        resolve_folder_kind("Archives", &[], None),
        FolderKind::Archive
    );
    assert_eq!(
        resolve_folder_kind("Junk Email", &[], None),
        FolderKind::Spam
    );
    assert_eq!(resolve_folder_kind("Spam", &[], None), FolderKind::Spam);
    assert_eq!(resolve_folder_kind("Personal", &[], None), FolderKind::User);
    assert_eq!(
        resolve_folder_kind("Projects/2026", &[], None),
        FolderKind::User
    );
}

#[test]
fn test_resolve_parent_hierarchy() {
    assert_eq!(
        resolve_parent("Work/Projects", Some("/")),
        Some(RemoteId::from("Work"))
    );
    assert_eq!(
        resolve_parent("Work/Projects/Rail", Some("/")),
        Some(RemoteId::from("Work/Projects"))
    );
    assert_eq!(
        resolve_parent("Archive.2024", Some(".")),
        Some(RemoteId::from("Archive"))
    );
    assert_eq!(resolve_parent("INBOX", Some("/")), None);
    assert_eq!(resolve_parent("Flat", None), None);
    assert_eq!(resolve_parent("Flat", Some("")), None);
}

#[test]
fn test_discovered_folders_helpers() {
    let folders = vec![
        RemoteFolder {
            remote_id: RemoteId::from("INBOX"),
            name: "INBOX".into(),
            kind: FolderKind::Inbox,
            parent: None,
        },
        RemoteFolder {
            remote_id: RemoteId::from("Sent Mail"),
            name: "Sent Mail".into(),
            kind: FolderKind::Sent,
            parent: None,
        },
        RemoteFolder {
            remote_id: RemoteId::from("Drafts"),
            name: "Drafts".into(),
            kind: FolderKind::Drafts,
            parent: None,
        },
    ];

    let disc = DiscoveredFolders::new(folders);
    assert_eq!(disc.sent_mailbox_name(), "Sent Mail");
    assert_eq!(disc.drafts_mailbox_name(), "Drafts");
    assert_eq!(disc.trash_mailbox_name(), None); // No trash folder scenario
    assert_eq!(disc.archive_mailbox_name(), None);
    assert!(disc.find_by_name("inbox").is_some());
    assert!(disc.find_by_id("Drafts").is_some());
}
