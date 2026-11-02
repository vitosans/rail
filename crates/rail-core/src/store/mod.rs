//! The local SQLite store. One file, WAL mode, FTS5.

pub mod accounts;
mod action_folders;
pub mod addresses;
#[cfg(test)]
mod addresses_tests;
mod apply_local;
#[cfg(test)]
mod apply_local_tests;
mod apply_single;
#[cfg(test)]
mod apply_single_tests;
pub mod drafts;
pub mod drafts_folder;
#[cfg(test)]
mod drafts_folder_tests;
#[cfg(test)]
mod drafts_tests;
pub mod filter;
pub mod migrations;
pub mod outbox;
mod outbox_drafts;
#[cfg(test)]
mod outbox_drafts_tests;
#[cfg(test)]
mod outbox_tests;
mod rollup;
#[cfg(test)]
mod search_text_tests;
pub mod snoozes;
#[cfg(test)]
mod snoozes_edge_tests;
#[cfg(test)]
mod snoozes_return_tests;
#[cfg(test)]
mod snoozes_single_tests;
#[cfg(test)]
mod snoozes_tests;
#[cfg(test)]
mod snoozes_view_tests;
pub mod sync_read;
mod sync_write;
#[cfg(test)]
mod sync_write_tests;
mod targets;
#[cfg(test)]
mod targets_tests;
pub mod view;
#[cfg(test)]
mod view_attachments_tests;
mod view_labels;
mod view_saved;
#[cfg(test)]
mod view_saved_tests;
mod view_sidebar;
#[cfg(test)]
mod view_sidebar_tests;
mod view_targets;
#[cfg(test)]
mod view_targets_tests;
#[cfg(test)]
mod view_tests;
#[cfg(test)]
mod view_unified_tests;
#[cfg(test)]
mod vouched_tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use accounts::AccountRow;
pub use addresses::{AddressEntry, MAX_SUGGESTIONS};
pub use sync_read::{StoreCounts, StoredBody, StoredHit, SyncStateRow, fts_query};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::error::ErrorClass;
use crate::paths::{create_private_dir, create_private_file, restrict_to_owner};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot create {}: {source}", .path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "database schema version {found} is newer than this build supports ({supported}); upgrade rail"
    )]
    TooNew { found: i64, supported: i64 },
    #[error(
        "the store is at schema version {found}, not {expected}; start the daemon so it can set the store up"
    )]
    NotSetUp { found: i64, expected: i64 },
}

impl StoreError {
    /// Busy or locked databases clear up on their own, and so does an older store: the
    /// daemon's writer migrates it when it opens it. Everything else will not, a store newer
    /// than this build included.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Sqlite(e)
                if matches!(
                    e.sqlite_error_code(),
                    Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                ) =>
            {
                ErrorClass::Transient
            }
            Self::NotSetUp { found, expected } if found < expected => ErrorClass::Transient,
            Self::Sqlite(_)
            | Self::CreateDir { .. }
            | Self::TooNew { .. }
            | Self::NotSetUp { .. } => ErrorClass::Permanent,
        }
    }
}

/// How long any connection waits for another's write lock before it reports busy. The writer
/// commits a batch in milliseconds; the account pipeline's upserts are as short.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            create_private_dir(parent).map_err(|source| StoreError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        create_private_file(path).map_err(|source| StoreError::CreateDir {
            path: path.to_path_buf(),
            source,
        })?;
        let conn = Connection::open(path)?;
        restrict_to_owner(path).map_err(|source| StoreError::CreateDir {
            path: path.to_path_buf(),
            source,
        })?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    /// A store at `path` migrated only up to `version`, as an older build left it, with no
    /// version check. For upgrade tests here and in the binary's crate; never in a build that
    /// ships (the `test-gates` feature).
    #[cfg(any(test, feature = "test-gates"))]
    pub fn open_at_version(path: &Path, version: i64) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrations::migrate_up_to(&mut conn, version)?;
        Ok(Self { conn })
    }

    /// A connection that can only read, for the sync engine and search. The database must
    /// already exist and be migrated (the daemon opens it with `Store::open` first); a missing
    /// file is an error, never a new empty database.
    pub fn open_reader(path: &Path) -> Result<Self, StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(path, flags)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "query_only", "ON")?;
        let found: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if found != migrations::LATEST_VERSION {
            return Err(StoreError::NotSetUp {
                found,
                expected: migrations::LATEST_VERSION,
            });
        }
        Ok(Self { conn })
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Hand the connection back; used by tests and by the Phase 2 writer actor.
    pub fn into_connection(self) -> Connection {
        self.conn
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{OptionalExtension, params};

    use crate::ops::OpError;

    fn table_names(store: &Store) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type IN ('table') AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn fresh_file_store_is_migrated_and_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("rail.db");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let mode: String = store
            .conn()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = store
            .conn()
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn reopen_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        let first = Store::open(&path).unwrap();
        first
            .conn()
            .execute(
                "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com')",
                [],
            )
            .unwrap();
        drop(first);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn schema_has_every_table_from_the_spec() {
        let store = Store::open_in_memory().unwrap();
        let names = table_names(&store);
        for expected in [
            "accounts",
            "folders",
            "threads",
            "messages",
            "message_folders",
            "attachments",
            "addresses",
            "outbox",
            "messages_fts",
            "snoozes",
            "snooze_messages",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing {expected} in {names:?}"
            );
        }
    }

    #[test]
    fn newer_schema_is_refused() {
        let store = Store::open_in_memory().unwrap();
        store
            .conn()
            .pragma_update(None, "user_version", 999)
            .unwrap();
        let err = migrations::migrate(&mut store.into_connection()).unwrap_err();
        assert!(
            matches!(err, StoreError::TooNew { found: 999, .. }),
            "{err}"
        );
    }

    fn seed_message(store: &Store, subject: &str) -> i64 {
        let c = store.conn();
        c.execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO threads (account_id, remote_id) VALUES (1, 't1')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO messages (account_id, thread_id, remote_id, subject, from_json, from_text,
               body_text, internal_date)
             VALUES (1, 1, 'm1', ?1, '[{\"name\":\"Dana\",\"address\":\"dana@example.com\"}]',
               'Dana dana@example.com', 'hello world', 1000)",
            params![subject],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn fts_hits(store: &Store, query: &str) -> Vec<i64> {
        let mut stmt = store
            .conn()
            .prepare("SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1")
            .unwrap();
        stmt.query_map(params![query], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn fts_follows_inserts_updates_and_deletes() {
        let store = Store::open_in_memory().unwrap();
        let id = seed_message(&store, "quarterly roadmap");
        assert_eq!(fts_hits(&store, "roadmap"), vec![id]);
        assert_eq!(fts_hits(&store, "dana"), vec![id]);
        assert_eq!(fts_hits(&store, "hello"), vec![id]);

        store
            .conn()
            .execute(
                "UPDATE messages SET subject = 'offsite plans' WHERE id = ?1",
                params![id],
            )
            .unwrap();
        assert!(fts_hits(&store, "roadmap").is_empty());
        assert_eq!(fts_hits(&store, "offsite"), vec![id]);

        store
            .conn()
            .execute("DELETE FROM messages WHERE id = ?1", params![id])
            .unwrap();
        assert!(fts_hits(&store, "offsite").is_empty());
    }

    #[test]
    fn fts_update_trigger_fires_only_for_indexed_columns() {
        let store = Store::open_in_memory().unwrap();
        let sql: String = store
            .conn()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'messages_fts_update'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("AFTER UPDATE OF subject, from_text, to_text, body_text ON messages"),
            "{sql}"
        );
        // A flag change leaves the index alone and the message still matches.
        let id = seed_message(&store, "quarterly roadmap");
        store
            .conn()
            .execute("UPDATE messages SET is_read = 1 WHERE id = ?1", params![id])
            .unwrap();
        assert_eq!(fts_hits(&store, "roadmap"), vec![id]);
    }

    #[test]
    fn attachments_are_indexed_by_message() {
        let store = Store::open_in_memory().unwrap();
        let indexed: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM pragma_index_list('attachments') l
                 JOIN pragma_index_info(l.name) i WHERE i.name = 'message_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn deleting_an_account_cascades() {
        let store = Store::open_in_memory().unwrap();
        seed_message(&store, "x");
        store
            .conn()
            .execute("DELETE FROM accounts WHERE id = 1", [])
            .unwrap();
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn errors_are_classified_for_retry() {
        let too_new = StoreError::TooNew {
            found: 9,
            supported: 1,
        };
        assert_eq!(too_new.class(), ErrorClass::Permanent);

        let busy = StoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        ));
        assert_eq!(busy.class(), ErrorClass::Transient);

        let constraint = StoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            None,
        ));
        assert_eq!(constraint.class(), ErrorClass::Permanent);

        // An older store is migrated by the daemon's writer when it opens it: waiting mends it.
        let older = StoreError::NotSetUp {
            found: 4,
            expected: 6,
        };
        assert_eq!(older.class(), ErrorClass::Transient);
        assert_eq!(OpError::from(older).class, ErrorClass::Transient);
        let unmigrated = StoreError::NotSetUp {
            found: 0,
            expected: 6,
        };
        assert_eq!(unmigrated.class(), ErrorClass::Transient);
        // A newer store needs a newer build; no retry mends that.
        let newer = StoreError::NotSetUp {
            found: 7,
            expected: 6,
        };
        assert_eq!(newer.class(), ErrorClass::Permanent);
    }

    #[cfg(unix)]
    #[test]
    fn database_and_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let path = data_dir.join("rail.db");
        let store = Store::open(&path).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com')",
                [],
            )
            .unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&data_dir), 0o700);
        let wal = data_dir.join("rail.db-wal");
        assert!(wal.exists(), "expected a WAL file after a write");
        assert_eq!(mode(&wal), 0o600);
        let shm = data_dir.join("rail.db-shm");
        assert!(shm.exists(), "expected a shared-memory file in WAL mode");
        assert_eq!(mode(&shm), 0o600);
    }

    #[test]
    fn migration_0002_adds_the_sync_columns_with_their_defaults() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        store
            .conn()
            .execute_batch(
                "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com');
                 INSERT INTO threads (account_id, remote_id) VALUES (1, 't1');
                 INSERT INTO messages (account_id, thread_id, remote_id) VALUES (1, 1, 'm1');",
            )
            .unwrap();
        let (origin, internal, searched, synced): (String, i64, Option<i64>, Option<i64>) = store
            .conn()
            .query_row(
                "SELECT origin, internal_date, searched_at, synced_at FROM messages",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (origin.as_str(), internal, searched, synced),
            ("sync", 0, None, None)
        );
        let (phase, needs_auth): (String, i64) = store
            .conn()
            .query_row("SELECT sync_phase, needs_auth FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((phase.as_str(), needs_auth), ("idle", 0));
        let bad = store
            .conn()
            .execute("UPDATE messages SET origin = 'other'", []);
        assert!(bad.is_err(), "origin is checked");
    }

    #[test]
    fn a_version_1_database_upgrades_and_keeps_its_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("migrations/0001_init.sql"))
            .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com');
             INSERT INTO threads (account_id, remote_id) VALUES (1, 't1');
             INSERT INTO messages (account_id, thread_id, remote_id, subject, date) VALUES (1, 1, 'm1', 'old', 1700000000);",
        )
        .unwrap();
        migrations::migrate(&mut conn).unwrap();
        let (subject, origin, internal): (String, String, i64) = conn
            .query_row(
                "SELECT subject, origin, internal_date FROM messages",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((subject.as_str(), origin.as_str()), ("old", "sync"));
        assert_eq!(internal, 1_700_000_000_000, "seconds become milliseconds");
    }

    /// 0003 adds the resume token columns to a version 2 database and keeps its
    /// backfill state; the token columns start empty.
    #[test]
    fn a_version_2_database_upgrades_to_3_and_keeps_its_backfill_state() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("migrations/0001_init.sql"))
            .unwrap();
        conn.execute_batch(include_str!("migrations/0002_sync.sql"))
            .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (name, kind, address, backfill_before, backfill_cursor)
               VALUES ('a', 'gmail', 'a@example.com', 1700000000000, '42');",
        )
        .unwrap();
        migrations::migrate(&mut conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, migrations::LATEST_VERSION);
        let store = Store { conn };
        let state = store.sync_state(1).unwrap();
        assert_eq!(state.backfill_before, Some(1_700_000_000_000));
        assert_eq!(state.backfill_cursor.as_deref(), Some("42"));
        assert_eq!(state.backfill_resume, None);
    }

    #[test]
    fn every_connection_waits_for_a_busy_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        let store = Store::open(&path).unwrap();
        let reader = Store::open_reader(&path).unwrap();
        for conn in [store.conn(), reader.conn()] {
            let ms: i64 = conn
                .pragma_query_value(None, "busy_timeout", |r| r.get(0))
                .unwrap();
            assert_eq!(ms, 5000);
        }
        let only: i64 = reader
            .conn()
            .pragma_query_value(None, "query_only", |r| r.get(0))
            .unwrap();
        assert_eq!(only, 1);
        assert!(reader.conn().execute("DELETE FROM accounts", []).is_err());
    }

    #[test]
    fn open_reader_refuses_an_unmigrated_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        Connection::open(&path).unwrap();
        let err = Store::open_reader(&path)
            .err()
            .expect("unmigrated is an error");
        assert!(
            matches!(err, StoreError::NotSetUp { found: 0, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("set the store up"), "{err}");
    }

    #[test]
    fn open_reader_does_not_create_a_missing_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        let err = Store::open_reader(&path)
            .err()
            .expect("a missing database is an error");
        assert!(matches!(err, StoreError::Sqlite(_)), "{err}");
        assert!(!path.exists(), "no file was created");
        assert!(!dir.path().join("rail.db-wal").exists());
    }

    #[test]
    fn version_4_adds_links_and_the_date_index_and_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            for (version, sql) in [
                (1, include_str!("migrations/0001_init.sql")),
                (2, include_str!("migrations/0002_sync.sql")),
                (3, include_str!("migrations/0003_backfill_token.sql")),
            ] {
                conn.execute_batch(sql).unwrap();
                conn.pragma_update(None, "user_version", version).unwrap();
            }
            conn.execute(
                "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com')",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let links: Option<String> = store
            .conn()
            .query_row("SELECT links FROM messages LIMIT 1", [], |r| r.get(0))
            .optional()
            .unwrap()
            .flatten();
        assert_eq!(links, None);
        let index: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'threads_by_date'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
    }

    /// 0006 adds the outbox's scheduling columns to a version-5 store and keeps its rows.
    #[test]
    fn version_6_adds_the_outbox_columns_and_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        {
            let store = Store::open_at_version(&path, 5).unwrap();
            store
                .conn()
                .execute_batch(
                    "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com');
                     INSERT INTO outbox (account_id, kind, payload, client_token)
                       VALUES (1, 'move', '{\"kind\":\"archive\"}', 'tok');",
                )
                .unwrap();
            assert_eq!(store.schema_version().unwrap(), 5);
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let (not_before, threads, inflight, reversal): (i64, String, Option<i64>, String) = store
            .conn()
            .query_row(
                "SELECT not_before, thread_ids, inflight_at, reversal FROM outbox",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (not_before, threads.as_str(), inflight, reversal.as_str()),
            (0, "[]", None, "[]")
        );
        let index: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'outbox_by_due'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
    }

    /// 0007 adds drafts, `messages.reply_to_json` and `outbox.draft_id` to a version-6 store
    /// and keeps its rows.
    #[test]
    fn version_7_adds_drafts_and_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        {
            let store = Store::open_at_version(&path, 6).unwrap();
            store
                .conn()
                .execute_batch(
                    "INSERT INTO accounts (name, kind, address) VALUES ('a', 'gmail', 'a@example.com');
                     INSERT INTO threads (account_id, remote_id) VALUES (1, 't1');
                     INSERT INTO messages (account_id, thread_id, remote_id, date, internal_date)
                       VALUES (1, 1, 'm1', 0, 0);
                     INSERT INTO outbox (account_id, kind, payload, client_token)
                       VALUES (1, 'move', '{\"kind\":\"archive\"}', 'tok');",
                )
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let draft: Option<i64> = store
            .conn()
            .query_row("SELECT draft_id FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(draft, None);
        let reply_to: String = store
            .conn()
            .query_row("SELECT reply_to_json FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reply_to, "[]");
        let drafts: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(drafts, 0);
    }

    /// 0008 adds `drafts.source_remote_id` to a version-7 store and keeps its drafts, the new
    /// column empty; attachments become plain files, and a Microsoft 365 message with any is
    /// marked unfetched (its text kept) so the next fetch says which they are.
    #[test]
    fn version_8_adds_the_drafts_source_and_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        {
            let store = Store::open_at_version(&path, 7).unwrap();
            store
                .conn()
                .execute_batch(
                    "INSERT INTO accounts (name, kind, address) VALUES ('a', 'm365', 'a@example.com');
                     INSERT INTO drafts (account_id, kind, message_id, created_at, updated_at)
                       VALUES (1, 'reply', '<d1@example.com>', 0, 0);
                     INSERT INTO accounts (name, kind, address) VALUES ('g', 'gmail', 'g@example.com');
                     INSERT INTO threads (account_id, remote_id) VALUES (1, 't1'), (2, 't2');
                     INSERT INTO messages (account_id, thread_id, remote_id, body_text, body_fetched_at)
                       VALUES (1, 1, 'm1', 'kept', 5), (2, 2, 'm2', 'kept', 5);
                     INSERT INTO attachments (message_id, remote_part_id) VALUES (1, 'a1'), (2, 'a2');",
                )
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), migrations::LATEST_VERSION);
        let (kind, source): (String, Option<String>) = store
            .conn()
            .query_row("SELECT kind, source_remote_id FROM drafts", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((kind.as_str(), source), ("reply", None));
        let rows: Vec<(String, Option<i64>, i64)> = store
            .conn()
            .prepare(
                "SELECT m.body_text, m.body_fetched_at, a.plain_file FROM messages m
                      JOIN attachments a ON a.message_id = m.id ORDER BY m.id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            [("kept".into(), None, 1), ("kept".into(), Some(5), 1)]
        );
    }
}
