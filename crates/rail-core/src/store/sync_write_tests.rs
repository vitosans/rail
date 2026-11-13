//! `Store::apply`: one transaction per batch, rollups, folders, FTS, sweep, prune, state.

use rusqlite::params;

use super::Store;
use crate::provider::mock::envelope;
use crate::provider::{Body, Envelope, FolderKind, PageToken, Relabel, RemoteFolder, Window};
use crate::sync::batch::{Batch, Origin, Prune, ResumeToken, StateUpdate, Sweep};

const DAY_MS: i64 = 86_400_000;
const NOW: i64 = 200 * 86_400;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('w', 'gmail', 'w@example.com')",
            [],
        )
        .unwrap();
    store
}

fn upserts(envelopes: Vec<Envelope>) -> Batch {
    Batch {
        upserts: envelopes,
        ..Batch::new(1)
    }
}

fn thread(store: &Store, remote: &str) -> Option<(i64, i64, i64, String, i64)> {
    store
        .conn()
        .query_row(
            "SELECT message_count, unread_count, is_starred, subject, last_date FROM threads WHERE remote_id = ?1",
            params![remote],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok()
}

fn folders_of(store: &Store, message: &str) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT f.remote_id FROM message_folders mf JOIN folders f ON f.id = mf.folder_id
             JOIN messages m ON m.id = mf.message_id WHERE m.remote_id = ?1 ORDER BY f.remote_id",
        )
        .unwrap();
    stmt.query_map(params![message], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn ids(store: &Store) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT remote_id FROM messages ORDER BY remote_id")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn upserts_create_threads_messages_folders_and_rollups() {
    let mut store = store();
    let mut second = envelope("m2", "t1", 20 * DAY_MS);
    second.is_read = true;
    second.subject = "re: first".into();
    let outcome = store
        .apply(
            &upserts(vec![envelope("m1", "t1", 10 * DAY_MS), second]),
            NOW,
        )
        .unwrap();
    assert_eq!(outcome.upserted, 2);
    assert_eq!(
        thread(&store, "t1"),
        Some((2, 1, 0, "subject m1".into(), 20 * 86_400))
    );
    assert_eq!(folders_of(&store, "m1"), ["INBOX"]);
    let participants: String = store
        .conn()
        .query_row("SELECT participants FROM threads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        participants,
        r#"[{"name":"Sender","address":"sender@example.com"}]"#
    );
    let (origin, synced): (String, i64) = store
        .conn()
        .query_row(
            "SELECT origin, synced_at FROM messages WHERE remote_id = 'm1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((origin.as_str(), synced), ("sync", NOW));
}

#[test]
fn relabels_and_deletes_keep_rollups_right_and_drop_empty_threads() {
    let mut store = store();
    store
        .apply(
            &upserts(vec![
                envelope("m1", "t1", DAY_MS),
                envelope("m2", "t1", 2 * DAY_MS),
            ]),
            NOW,
        )
        .unwrap();
    let relabel = Relabel {
        remote_id: "m1".into(),
        add_folders: vec!["Label_1".into()],
        remove_folders: vec!["INBOX".into()],
        read: Some(true),
        starred: Some(true),
    };
    let unknown = Relabel {
        remote_id: "never-seen".into(),
        read: Some(true),
        ..Relabel::default()
    };
    store
        .apply(
            &Batch {
                relabels: vec![relabel, unknown],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert_eq!(folders_of(&store, "m1"), ["Label_1"]);
    assert_eq!(
        thread(&store, "t1").map(|t| (t.0, t.1, t.2)),
        Some((2, 1, 1))
    );
    let gone = store
        .apply(
            &Batch {
                deletes: vec!["m1".into(), "m2".into(), "nope".into()],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert_eq!(gone.deleted, 2);
    assert_eq!(thread(&store, "t1"), None, "an empty thread is deleted");
}

#[test]
fn fts_follows_upserts_bodies_and_deletes() {
    let mut store = store();
    store
        .apply(&upserts(vec![envelope("m1", "t1", DAY_MS)]), NOW)
        .unwrap();
    assert_eq!(store.search_local("subject", None, 10).unwrap().len(), 1);
    let body = Body {
        text: "the quarterly roadmap".into(),
        ..Body::default()
    };
    store
        .apply(
            &Batch {
                bodies: vec![("m1".into(), body)],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert_eq!(store.search_local("roadmap", None, 10).unwrap().len(), 1);
    let mut renamed = envelope("m1", "t1", DAY_MS);
    renamed.subject = "offsite".into();
    store.apply(&upserts(vec![renamed]), NOW).unwrap();
    assert!(store.search_local("subject", None, 10).unwrap().is_empty());
    assert_eq!(store.search_local("offsite", None, 10).unwrap().len(), 1);
    store
        .apply(
            &Batch {
                deletes: vec!["m1".into()],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert!(store.search_local("offsite", None, 10).unwrap().is_empty());
}

/// Review focus 3.
#[test]
fn an_unseen_label_gets_a_placeholder_folder() {
    let mut store = store();
    let mut env = envelope("m1", "t1", DAY_MS);
    env.folders = vec!["INBOX".into(), "Label_new".into()];
    store.apply(&upserts(vec![env]), NOW).unwrap();
    assert_eq!(folders_of(&store, "m1"), ["INBOX", "Label_new"]);
    let named = vec![
        RemoteFolder {
            parent: None,
            remote_id: "INBOX".into(),
            name: "Inbox".into(),
            kind: FolderKind::Inbox,
        },
        RemoteFolder {
            parent: None,
            remote_id: "Label_new".into(),
            name: "Projects".into(),
            kind: FolderKind::User,
        },
    ];
    store
        .apply(
            &Batch {
                folders: Some(named),
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    let (name, kind): (String, String) = store
        .conn()
        .query_row(
            "SELECT name, kind FROM folders WHERE remote_id = 'Label_new'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((name.as_str(), kind.as_str()), ("Projects", "user"));
}

/// Review focus 3.
#[test]
fn a_deleted_label_keeps_its_messages() {
    let mut store = store();
    let mut env = envelope("m1", "t1", DAY_MS);
    env.folders = vec!["INBOX".into(), "Label_old".into()];
    store.apply(&upserts(vec![env]), NOW).unwrap();
    let inbox_only = vec![RemoteFolder {
        parent: None,
        remote_id: "INBOX".into(),
        name: "Inbox".into(),
        kind: FolderKind::Inbox,
    }];
    store
        .apply(
            &Batch {
                folders: Some(inbox_only),
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert_eq!(ids(&store), ["m1"]);
    assert_eq!(folders_of(&store, "m1"), ["INBOX"]);
}

fn origin(store: &Store, id: &str) -> (String, Option<i64>) {
    store
        .conn()
        .query_row(
            "SELECT origin, searched_at FROM messages WHERE remote_id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn search_rows_never_downgrade_sync_rows_and_sync_promotes_search_rows() {
    let mut store = store();
    store
        .apply(&upserts(vec![envelope("m1", "t1", DAY_MS)]), NOW)
        .unwrap();
    let found = Batch {
        upserts: vec![envelope("m1", "t1", DAY_MS), envelope("old", "t9", DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&found, NOW + 5).unwrap();
    assert_eq!(origin(&store, "m1").0, "sync");
    let synced: Option<i64> = store
        .conn()
        .query_row(
            "SELECT synced_at FROM messages WHERE remote_id = 'm1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        synced,
        Some(NOW),
        "a search hit keeps a sync row's synced_at for the sweep"
    );
    assert_eq!(origin(&store, "old"), ("search".into(), Some(NOW + 5)));
    store
        .apply(
            &Batch {
                searched: vec!["old".into()],
                ..Batch::new(1)
            },
            NOW + 9,
        )
        .unwrap();
    assert_eq!(origin(&store, "old").1, Some(NOW + 9));
    store
        .apply(&upserts(vec![envelope("old", "t9", DAY_MS)]), NOW + 10)
        .unwrap();
    assert_eq!(origin(&store, "old").0, "sync");
}

#[test]
fn the_sweep_deletes_sync_mail_a_full_backfill_did_not_see_again() {
    let mut store = store();
    store
        .apply(
            &upserts(vec![
                envelope("kept", "t1", DAY_MS),
                envelope("gone", "t2", DAY_MS),
            ]),
            NOW,
        )
        .unwrap();
    let found = Batch {
        upserts: vec![envelope("hit", "t3", DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&found, NOW).unwrap();
    store
        .apply(&upserts(vec![envelope("kept", "t1", DAY_MS)]), NOW + 100)
        .unwrap();
    let sweep = Batch {
        sweep: Some(Sweep {
            started_at: NOW + 50,
            window_start_ms: None,
        }),
        ..Batch::new(1)
    };
    let outcome = store.apply(&sweep, NOW + 200).unwrap();
    assert_eq!(outcome.swept, 1);
    assert_eq!(
        ids(&store),
        ["hit", "kept"],
        "search hits are not the sweep's"
    );
    assert_eq!(thread(&store, "t2"), None);
}

#[test]
fn prune_keeps_starred_mail_and_threads_with_mail_inside_the_window() {
    let mut store = store();
    let mut starred = envelope("starred", "t2", DAY_MS);
    starred.is_starred = true;
    store
        .apply(
            &upserts(vec![
                envelope("old", "t1", DAY_MS),
                starred,
                envelope("old-in-live-thread", "t3", DAY_MS),
                envelope("recent", "t3", 150 * DAY_MS),
            ]),
            NOW,
        )
        .unwrap();
    let stale = Batch {
        upserts: vec![envelope("stale-hit", "t8", DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&stale, NOW - 20 * 86_400).unwrap();
    let fresh = Batch {
        upserts: vec![envelope("fresh-hit", "t9", DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&fresh, NOW).unwrap();
    let prune = Batch {
        prune: Some(Prune {
            window_start_ms: Some(100 * DAY_MS),
            search_before: NOW - 14 * 86_400,
        }),
        ..Batch::new(1)
    };
    let outcome = store.apply(&prune, NOW).unwrap();
    assert_eq!(outcome.pruned, 2);
    assert_eq!(
        ids(&store),
        ["fresh-hit", "old-in-live-thread", "recent", "starred"]
    );
    assert_eq!(thread(&store, "t1"), None);
}

fn resume_token() -> ResumeToken {
    ResumeToken {
        token: PageToken("0987654321".into()),
        listing: Window {
            after_ms: Some(3 * DAY_MS),
            before_ms: None,
        },
        issued_at: NOW - 5,
    }
}

#[test]
fn state_updates_set_and_clear_columns() {
    let mut store = store();
    let set = Batch {
        state: StateUpdate {
            cursor: Some(Some("77".into())),
            backfill_before: Some(Some(5)),
            backfill_cursor: Some(Some("70".into())),
            backfill_window: Some(Some(0)),
            backfill_started_at: Some(Some(9)),
            backfill_resume: Some(Some(resume_token())),
            phase: Some("backfill".into()),
            last_sync_at: Some(NOW),
            last_error: Some(Some(("transient".into(), NOW))),
            needs_auth: Some(true),
        },
        ..Batch::new(1)
    };
    store.apply(&set, NOW).unwrap();
    let state = store.sync_state(1).unwrap();
    assert_eq!(state.cursor.as_deref(), Some("77"));
    assert_eq!(
        (
            state.backfill_before,
            state.backfill_window,
            state.backfill_started_at
        ),
        (Some(5), Some(0), Some(9))
    );
    assert_eq!(state.backfill_cursor.as_deref(), Some("70"));
    assert_eq!(state.backfill_resume, Some(resume_token()));
    assert_eq!(
        (state.phase.as_str(), state.last_sync_at, state.needs_auth),
        ("backfill", Some(NOW), true)
    );
    assert_eq!(state.last_error_code.as_deref(), Some("transient"));
    let clear = Batch {
        state: StateUpdate {
            backfill_before: Some(None),
            backfill_resume: Some(None),
            last_error: Some(None),
            needs_auth: Some(false),
            ..StateUpdate::default()
        },
        ..Batch::new(1)
    };
    store.apply(&clear, NOW).unwrap();
    let state = store.sync_state(1).unwrap();
    assert_eq!(state.backfill_resume, None);
    assert_eq!(
        (
            state.backfill_before,
            state.last_error_code,
            state.needs_auth
        ),
        (None, None, false)
    );
    assert_eq!(
        state.cursor.as_deref(),
        Some("77"),
        "untouched columns stay"
    );
}

/// Spec §4: a crash or failure loses at most the batch in flight, and loses all of it: the
/// poison fires after every step (sweep, prune, rollups, state), just before commit.
#[test]
fn a_failing_batch_changes_nothing() {
    let mut store = store();
    store
        .apply(
            &upserts(vec![
                envelope("a", "ta", DAY_MS),
                envelope("b", "tb", 150 * DAY_MS),
            ]),
            NOW,
        )
        .unwrap();
    let before = (
        thread(&store, "ta"),
        thread(&store, "tb"),
        store.sync_state(1).unwrap(),
    );
    let poisoned = Batch {
        upserts: vec![envelope("c", "tc", 160 * DAY_MS)],
        relabels: vec![Relabel {
            remote_id: "b".into(),
            read: Some(true),
            starred: Some(true),
            ..Relabel::default()
        }],
        sweep: Some(Sweep {
            started_at: NOW + 1,
            window_start_ms: Some(100 * DAY_MS),
        }),
        prune: Some(Prune {
            window_start_ms: Some(100 * DAY_MS),
            search_before: NOW,
        }),
        state: StateUpdate {
            cursor: Some(Some("9".into())),
            backfill_before: Some(Some(5)),
            backfill_resume: Some(Some(resume_token())),
            phase: Some("backfill".into()),
            ..StateUpdate::default()
        },
        poison: true,
        ..Batch::new(1)
    };
    assert!(store.apply(&poisoned, NOW + 2).is_err());
    assert_eq!(
        ids(&store),
        ["a", "b"],
        "prune, sweep and upserts rolled back"
    );
    let after = (
        thread(&store, "ta"),
        thread(&store, "tb"),
        store.sync_state(1).unwrap(),
    );
    assert_eq!(
        after, before,
        "rollups and the cursor, marker, page token and phase rolled back"
    );
    assert_eq!(thread(&store, "tc"), None);
}

#[test]
fn bodies_record_attachments_for_the_thread_rollup_and_an_unknown_list_keeps_them() {
    let mut store = store();
    store
        .apply(&upserts(vec![envelope("m1", "t1", DAY_MS)]), NOW)
        .unwrap();
    let body = Body {
        text: "see attached".into(),
        html: Some("<p>see attached</p>".into()),
        attachments: vec![crate::provider::AttachmentMeta {
            part_id: "1".into(),
            filename: "a.pdf".into(),
            mime_type: "application/pdf".into(),
            size: 10,
            plain_file: true,
            content_id: None,
        }],
        ..Body::default()
    };
    store
        .apply(
            &Batch {
                bodies: vec![("m1".into(), body)],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    let (has, fetched): (i64, Option<i64>) = store
        .conn()
        .query_row(
            "SELECT t.has_attachments, m.body_fetched_at FROM threads t JOIN messages m ON m.thread_id = t.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((has, fetched), (1, Some(NOW)));
    // A list the provider could not give leaves the stored one as it is.
    let unknown = Body {
        text: "again".into(),
        attachments_unknown: true,
        ..Body::default()
    };
    let batch = Batch {
        bodies: vec![("m1".into(), unknown)],
        ..Batch::new(1)
    };
    store.apply(&batch, NOW).unwrap();
    let kept: (i64, String) = store
        .conn()
        .query_row(
            "SELECT count(*), (SELECT body_text FROM messages) FROM attachments",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kept, (1, "again".into()));
}

/// Spec §4 step 5: any message inside the window keeps its thread, a search hit included.
#[test]
fn prune_keeps_stale_search_hits_in_a_thread_with_any_message_inside_the_window() {
    let mut store = store();
    let hits = Batch {
        upserts: vec![
            envelope("stale-old", "t1", DAY_MS),
            envelope("stale-recent", "t1", 150 * DAY_MS),
            envelope("stale-alone", "t2", DAY_MS),
        ],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&hits, NOW - 20 * 86_400).unwrap();
    let prune = Batch {
        prune: Some(Prune {
            window_start_ms: Some(100 * DAY_MS),
            search_before: NOW - 14 * 86_400,
        }),
        ..Batch::new(1)
    };
    let outcome = store.apply(&prune, NOW).unwrap();
    assert_eq!(outcome.pruned, 1);
    assert_eq!(ids(&store), ["stale-old", "stale-recent"]);
    assert_eq!(thread(&store, "t2"), None);
}

/// A stored token Rail would not send reads as none, so the backfill falls back to its
/// marker instead of sending it.
#[test]
fn an_unsafe_stored_page_token_reads_as_none() {
    let store = store();
    store
        .conn()
        .execute(
            "UPDATE accounts SET backfill_page_token = 'a b&c', backfill_token_at = 1",
            [],
        )
        .unwrap();
    assert_eq!(store.sync_state(1).unwrap().backfill_resume, None);
}

/// One query says which ids of a page a running backfill already stored: sync rows
/// synced since it began; search hits and older rows are fetched again.
#[test]
fn backfilled_ids_are_sync_rows_synced_since_the_backfill_began() {
    let mut store = store();
    store
        .apply(&upserts(vec![envelope("early", "t1", DAY_MS)]), NOW - 100)
        .unwrap();
    store
        .apply(&upserts(vec![envelope("late", "t1", DAY_MS)]), NOW)
        .unwrap();
    let hit = Batch {
        upserts: vec![envelope("hit", "t2", DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&hit, NOW).unwrap();
    let page: Vec<String> = ["early", "late", "hit", "absent"]
        .map(String::from)
        .to_vec();
    let since = store.backfilled_ids(1, &page, Some(NOW - 10)).unwrap();
    assert_eq!(since.into_iter().collect::<Vec<_>>(), ["late"]);
    let mut any: Vec<String> = store
        .backfilled_ids(1, &page, None)
        .unwrap()
        .into_iter()
        .collect();
    any.sort_unstable();
    assert_eq!(any, ["early", "late"]);
    assert!(store.backfilled_ids(1, &[], None).unwrap().is_empty());
    assert!(store.backfilled_ids(2, &page, None).unwrap().is_empty());
}

/// The count a pass sizes its page ceiling with: the same rows as
/// `backfilled_ids`, inside the listing's receive times.
#[test]
fn backfilled_count_counts_sync_rows_inside_the_listing() {
    let mut store = store();
    store
        .apply(&upserts(vec![envelope("early", "t1", DAY_MS)]), NOW - 100)
        .unwrap();
    let recent = vec![
        envelope("a", "t1", 5 * DAY_MS),
        envelope("b", "t1", 9 * DAY_MS),
    ];
    store.apply(&upserts(recent), NOW).unwrap();
    let hit = Batch {
        upserts: vec![envelope("hit", "t2", 5 * DAY_MS)],
        origin: Origin::Search,
        ..Batch::new(1)
    };
    store.apply(&hit, NOW).unwrap();
    let all = Window::default();
    assert_eq!(store.backfilled_count(1, &all, None).unwrap(), 3);
    assert_eq!(store.backfilled_count(1, &all, Some(NOW - 10)).unwrap(), 2);
    let below = Window {
        after_ms: Some(2 * DAY_MS),
        before_ms: Some(9 * DAY_MS),
    };
    assert_eq!(store.backfilled_count(1, &below, None).unwrap(), 1);
    assert_eq!(store.backfilled_count(2, &all, None).unwrap(), 0);
}

#[test]
fn a_body_stores_its_links_as_json_and_none_as_null() {
    let mut store = store();
    store
        .apply(
            &upserts(vec![
                envelope("m1", "t1", DAY_MS),
                envelope("m2", "t1", DAY_MS),
            ]),
            NOW,
        )
        .unwrap();
    let with_links = Body {
        text: "see[1]".into(),
        links: vec!["https://a.example/\"q\"".into()],
        ..Body::default()
    };
    let without = Body {
        text: "plain".into(),
        ..Body::default()
    };
    store
        .apply(
            &Batch {
                bodies: vec![("m1".into(), with_links), ("m2".into(), without)],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    let links = |remote: &str| -> Option<String> {
        store
            .conn()
            .query_row(
                "SELECT links FROM messages WHERE remote_id = ?1",
                params![remote],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(
        links("m1").as_deref(),
        Some(r#"["https://a.example/\"q\""]"#)
    );
    assert_eq!(links("m2"), None);
}

#[test]
fn reply_to_is_stored_as_json_and_replaced_by_the_next_upsert() {
    let mut store = store();
    let reply_to = |address: &str| crate::provider::Mailbox {
        name: None,
        address: address.into(),
    };
    let first = Envelope {
        reply_to: vec![reply_to("list@example.com")],
        ..envelope("m1", "t1", DAY_MS)
    };
    store.apply(&upserts(vec![first]), NOW).unwrap();
    let stored = |store: &Store| -> String {
        store
            .conn()
            .query_row("SELECT reply_to_json FROM messages", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(
        stored(&store),
        r#"[{"name":null,"address":"list@example.com"}]"#
    );
    store
        .apply(&upserts(vec![envelope("m1", "t1", DAY_MS)]), NOW)
        .unwrap();
    assert_eq!(stored(&store), "[]", "no Reply-To any more");
}

fn parent_of(store: &Store, folder: &str) -> Option<String> {
    store
        .conn()
        .query_row(
            "SELECT p.remote_id FROM folders f LEFT JOIN folders p ON p.id = f.parent_id
             WHERE f.remote_id = ?1",
            params![folder],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_nested_folder_keeps_its_parent_even_when_listed_first() {
    let mut store = store();
    let folder = |id: &str, parent: Option<&str>| RemoteFolder {
        remote_id: id.into(),
        name: id.into(),
        kind: FolderKind::User,
        parent: parent.map(Into::into),
    };
    let nested = Batch {
        folders: Some(vec![folder("child", Some("top")), folder("top", None)]),
        ..Batch::new(1)
    };
    store.apply(&nested, NOW).unwrap();
    assert_eq!(parent_of(&store, "child").as_deref(), Some("top"));
    assert_eq!(parent_of(&store, "top"), None);
    let flat = Batch {
        folders: Some(vec![folder("child", None), folder("top", None)]),
        ..Batch::new(1)
    };
    store.apply(&flat, NOW).unwrap();
    assert_eq!(parent_of(&store, "child"), None, "moved to the top");
}

#[test]
fn a_move_records_the_messages_new_provider_id() {
    let mut store = store();
    store
        .apply(
            &upserts(vec![envelope("INBOX:100:42", "t1", 10 * DAY_MS)]),
            NOW,
        )
        .unwrap();
    store
        .apply(
            &Batch {
                moves: vec![("INBOX:100:42".into(), "Archive:777:99".into())],
                ..Batch::new(1)
            },
            NOW,
        )
        .unwrap();
    assert_eq!(ids(&store), vec!["Archive:777:99".to_string()]);
    assert!(thread(&store, "t1").is_some());
}
