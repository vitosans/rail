use super::*;

#[test]
fn stacked_label_undo_replays_into_the_original_accounts_recreated_folder() {
    let mut store = seeded();
    let t1 = thread(&store, "t1");
    let label = folder(&store, "Label_1");
    let addition = enqueue(
        &mut store,
        Action::Labels {
            add: vec![label],
            remove: vec![],
        },
        vec![t1],
        "add",
    );
    let removal = enqueue(
        &mut store,
        Action::Labels {
            add: vec![],
            remove: vec![label],
        },
        vec![t1],
        "remove",
    );
    store.conn().execute_batch("INSERT INTO accounts (name, kind, address) VALUES ('other', 'gmail', 'other@example.com');
        UPDATE folders SET account_id = 2 WHERE remote_id = 'Label_1';
        INSERT INTO folders (account_id, remote_id, name, kind) VALUES (1, 'Label_1', 'Projects', 'user');").unwrap();
    let recreated = store.conn().last_insert_rowid();
    undo(&mut store, vec![removal]);
    assert_eq!(
        members(&store, label),
        0,
        "another account gets no memberships"
    );
    assert_eq!(
        members(&store, recreated),
        2,
        "the older add is replayed by provider identity"
    );
    undo(&mut store, vec![addition]);
    assert_eq!(members(&store, label), 0);
    assert_eq!(members(&store, recreated), 0);
}

#[test]
fn upgrading_keeps_the_action_identity_after_its_reversal_is_restacked() {
    let mut store = seeded();
    let t1 = thread(&store, "t1");
    let label = folder(&store, "Label_1");
    let addition = enqueue(
        &mut store,
        Action::Labels {
            add: vec![label],
            remove: vec![],
        },
        vec![t1],
        "add",
    );
    let read = enqueue(&mut store, Action::Read { on: true }, vec![t1], "read");
    store.conn().execute_batch("ALTER TABLE outbox DROP COLUMN relay_accepted_at; ALTER TABLE outbox DROP COLUMN folder_remotes; ALTER TABLE attachments DROP COLUMN content_id; ALTER TABLE messages DROP COLUMN vouched_domains; PRAGMA user_version = 8;
        UPDATE folders SET remote_id = 'Label_other' WHERE remote_id = 'Label_1';
        INSERT INTO folders (account_id, remote_id, name, kind) VALUES (1, 'Label_1', 'Projects', 'user');").unwrap();
    let recreated = store.conn().last_insert_rowid();
    super::super::migrations::migrate(&mut store.conn).unwrap();
    undo(&mut store, vec![read]);
    let row = store.outbox_row(addition).unwrap().unwrap();
    assert_eq!(
        store.row_action(&row).unwrap(),
        Some(Action::Labels {
            add: vec![recreated],
            remove: vec![]
        })
    );
}

#[test]
fn a_noop_label_action_keeps_its_identity_when_a_local_id_is_reused() {
    let mut store = seeded();
    let t1 = thread(&store, "t1");
    let label = folder(&store, "Label_1");
    let removal = enqueue(
        &mut store,
        Action::Labels {
            add: vec![],
            remove: vec![label],
        },
        vec![t1],
        "remove",
    );
    assert!(
        store
            .outbox_row(removal)
            .unwrap()
            .unwrap()
            .reversal
            .is_empty()
    );
    store.conn().execute_batch("UPDATE folders SET remote_id = 'Label_other' WHERE remote_id = 'Label_1';
        INSERT INTO folders (account_id, remote_id, name, kind) VALUES (1, 'Label_1', 'Projects', 'user');").unwrap();
    let recreated = store.conn().last_insert_rowid();
    let row = store.outbox_row(removal).unwrap().unwrap();
    assert_eq!(
        store.row_action(&row).unwrap(),
        Some(Action::Labels {
            add: vec![],
            remove: vec![recreated]
        })
    );
    store
        .conn()
        .execute("DELETE FROM folders WHERE id = ?1", [recreated])
        .unwrap();
    assert_eq!(
        store.row_action(&row).unwrap(),
        None,
        "a reused id cannot stand in for a deleted label"
    );
}

#[test]
fn sync_reapplies_a_label_using_its_account_and_provider_identity() {
    let mut store = seeded();
    let t1 = thread(&store, "t1");
    let label = folder(&store, "Label_1");
    let addition = enqueue(
        &mut store,
        Action::Labels {
            add: vec![label],
            remove: vec![],
        },
        vec![t1],
        "add",
    );
    store.conn().execute_batch("INSERT INTO accounts (name, kind, address) VALUES ('other', 'gmail', 'other@example.com');
        UPDATE folders SET account_id = 2 WHERE remote_id = 'Label_1';
        INSERT INTO folders (account_id, remote_id, name, kind) VALUES (1, 'Label_1', 'Projects', 'user');
        DELETE FROM message_folders WHERE folder_id IN (SELECT id FROM folders WHERE account_id = 2);").unwrap();
    let recreated = store.conn().last_insert_rowid();
    store
        .apply(
            &Batch {
                upserts: vec![envelope("m1", "t1", 1_000), envelope("m2", "t1", 2_000)],
                ..Batch::new(1)
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(members(&store, label), 0);
    assert_eq!(members(&store, recreated), 2);
    undo(&mut store, vec![addition]);
    assert_eq!(members(&store, label), 0);
    assert_eq!(members(&store, recreated), 0);
}
