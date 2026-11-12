-- Rail schema v13: widen accounts.kind check constraint to allow 'imapsmtp'
-- alongside 'gmail' and 'm365'.
CREATE TABLE accounts_new (
  id                      INTEGER PRIMARY KEY,
  name                    TEXT NOT NULL,
  kind                    TEXT NOT NULL CHECK (kind IN ('gmail', 'm365', 'imapsmtp')),
  address                 TEXT NOT NULL UNIQUE,
  display_name            TEXT,
  color                   TEXT,
  sync_cursor             TEXT,
  last_sync_at            INTEGER,
  created_at              INTEGER NOT NULL DEFAULT (unixepoch()),
  sync_phase              TEXT NOT NULL DEFAULT 'idle',
  last_error_code         TEXT,
  last_error_at           INTEGER,
  needs_auth              INTEGER NOT NULL DEFAULT 0,
  backfill_before         INTEGER,
  backfill_cursor         TEXT,
  backfill_window         INTEGER,
  backfill_started_at     INTEGER,
  backfill_page_token     TEXT,
  backfill_list_after_ms  INTEGER,
  backfill_list_before_ms INTEGER,
  backfill_token_at       INTEGER
);

INSERT INTO accounts_new (
  id, name, kind, address, display_name, color, sync_cursor, last_sync_at, created_at,
  sync_phase, last_error_code, last_error_at, needs_auth, backfill_before, backfill_cursor,
  backfill_window, backfill_started_at, backfill_page_token, backfill_list_after_ms,
  backfill_list_before_ms, backfill_token_at
)
SELECT
  id, name, kind, address, display_name, color, sync_cursor, last_sync_at, created_at,
  sync_phase, last_error_code, last_error_at, needs_auth, backfill_before, backfill_cursor,
  backfill_window, backfill_started_at, backfill_page_token, backfill_list_after_ms,
  backfill_list_before_ms, backfill_token_at
FROM accounts;

DROP TABLE accounts;

ALTER TABLE accounts_new RENAME TO accounts;
