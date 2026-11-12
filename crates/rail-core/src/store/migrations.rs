use rusqlite::{Connection, TransactionBehavior};

use super::StoreError;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_sync.sql")),
    (3, include_str!("migrations/0003_backfill_token.sql")),
    (4, include_str!("migrations/0004_view.sql")),
    (5, include_str!("migrations/0005_search_text.sql")),
    (6, include_str!("migrations/0006_actions.sql")),
    (7, include_str!("migrations/0007_compose.sql")),
    (8, include_str!("migrations/0008_draft_source.sql")),
    (9, include_str!("migrations/0009_action_folders.sql")),
    (10, include_str!("migrations/0010_snoozes.sql")),
    (11, include_str!("migrations/0011_content_id.sql")),
    (12, include_str!("migrations/0012_vouched_domains.sql")),
    (13, include_str!("migrations/0013_imapsmtp.sql")),
];

pub const LATEST_VERSION: i64 = 13;

/// Apply every migration newer than the database's `user_version`, each in its own transaction.
pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    migrate_up_to(conn, LATEST_VERSION)
}

/// `migrate`, stopping after version `target`. Each migration and its `user_version` bump
/// commit together, so a reader sees one version or the next, never a half-applied one.
pub(crate) fn migrate_up_to(conn: &mut Connection, target: i64) -> Result<(), StoreError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > LATEST_VERSION {
        return Err(StoreError::TooNew {
            found: current,
            supported: LATEST_VERSION,
        });
    }
    let fk_enabled: bool = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    if fk_enabled {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
    }
    for (version, sql) in MIGRATIONS {
        if *version > target {
            break;
        }
        // Take the write lock before reading `user_version`, so a concurrent opener
        // waits here and then sees the version we committed instead of re-applying.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if current > LATEST_VERSION {
            return Err(StoreError::TooNew {
                found: current,
                supported: LATEST_VERSION,
            });
        }
        if *version <= current {
            continue;
        }
        tracing::info!(version, "applying store migration");
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    if fk_enabled {
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn migration_thirteen_widens_accounts_kind_and_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rail.db");
        {
            let store = Store::open_at_version(&path, 12).unwrap();
            store
                .conn()
                .execute_batch(
                    "INSERT INTO accounts (name, kind, address) VALUES ('work', 'gmail', 'w@example.com');
                     INSERT INTO threads (account_id, remote_id) VALUES (1, 't1');
                     INSERT INTO messages (account_id, thread_id, remote_id) VALUES (1, 1, 'm1');",
                )
                .unwrap();
            // Kind 'imapsmtp' must fail on version 12
            assert!(
                store
                    .conn()
                    .execute(
                        "INSERT INTO accounts (name, kind, address) VALUES ('imap', 'imapsmtp', 'i@e.com')",
                        [],
                    )
                    .is_err(),
                "version 12 check constraint rejects imapsmtp"
            );
        }

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_VERSION);

        // Verify existing rows survived
        let count: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM messages WHERE account_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify imapsmtp is accepted on version 13
        store
            .conn()
            .execute(
                "INSERT INTO accounts (name, kind, address) VALUES ('imap', 'imapsmtp', 'i@e.com')",
                [],
            )
            .unwrap();

        // Verify invalid kind is still rejected
        assert!(
            store
                .conn()
                .execute(
                    "INSERT INTO accounts (name, kind, address) VALUES ('pop', 'pop3', 'p@e.com')",
                    [],
                )
                .is_err()
        );
    }
}
