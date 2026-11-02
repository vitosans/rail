//! The From domains a body's receiving server vouched for: stored with the body, read back
//! with the message, and empty for mail stored before Rail kept them.

use super::Store;
use crate::provider::mock::envelope;
use crate::provider::{Body, FolderKind, RemoteFolder};
use crate::sync::batch::Batch;

const BASE_MS: i64 = 1_760_000_000_000;
const NOW: i64 = 1_760_100_000;

/// A Gmail account with one message, `m1` in thread `t1`, its body not yet fetched.
fn one_message(store: &mut Store) {
    store
        .conn()
        .execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('work', 'gmail', 'w@example.com')",
            [],
        )
        .unwrap();
    let folders = vec![RemoteFolder {
        parent: None,
        remote_id: "INBOX".into(),
        name: "Inbox".into(),
        kind: FolderKind::Inbox,
    }];
    let batch = Batch {
        folders: Some(folders),
        upserts: vec![envelope("m1", "t1", BASE_MS)],
        ..Batch::new(1)
    };
    store.apply(&batch, NOW).unwrap();
}

fn store_body(store: &mut Store, vouched: &[&str]) {
    let body = Body {
        text: "hi".into(),
        vouched_domains: vouched.iter().map(|d| (*d).to_string()).collect(),
        ..Body::default()
    };
    let batch = Batch {
        bodies: vec![("m1".into(), body)],
        ..Batch::new(1)
    };
    store.apply(&batch, NOW).unwrap();
}

fn vouched(store: &Store) -> Vec<String> {
    let thread_id: i64 = store
        .conn()
        .query_row("SELECT id FROM threads WHERE remote_id = 't1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let detail = store.thread_detail(thread_id).unwrap().expect("the thread");
    detail.messages[0].vouched_domains.clone()
}

#[test]
fn the_vouched_domains_are_stored_with_the_body_and_replaced_by_the_next_one() {
    let mut store = Store::open_in_memory().unwrap();
    one_message(&mut store);
    assert!(vouched(&store).is_empty(), "no body yet");
    store_body(&mut store, &["example.com", "mail.example.com"]);
    assert_eq!(vouched(&store), ["example.com", "mail.example.com"]);
    store_body(&mut store, &[]);
    assert!(vouched(&store).is_empty());
}

/// Migration 12 adds the column to a version-11 store; a body already there vouches for
/// nothing until it is fetched again, and an unreadable value reads as nothing.
#[test]
fn migration_twelve_adds_vouched_domains_to_a_version_eleven_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rail.db");
    {
        let store = Store::open_at_version(&path, 11).unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO accounts (name, kind, address) VALUES ('work', 'gmail', 'w@example.com');
                 INSERT INTO threads (account_id, remote_id) VALUES (1, 't1');
                 INSERT INTO messages (account_id, thread_id, remote_id, body_text, body_fetched_at)
                   VALUES (1, 1, 'm1', 'kept', 5);",
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.schema_version().unwrap(),
        super::migrations::LATEST_VERSION
    );
    assert!(vouched(&store).is_empty());
    store
        .conn()
        .execute("UPDATE messages SET vouched_domains = 'not json'", [])
        .unwrap();
    assert!(vouched(&store).is_empty());
}
