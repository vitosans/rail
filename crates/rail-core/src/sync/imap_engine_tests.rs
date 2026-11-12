//! Tests for ImapSmtpProvider integration with the Sync Engine.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use super::engine::{Engine, EngineParts};
use super::status::StatusBoard;
use super::writer::{WriterHandle, WriterTask, spawn_writer};
use crate::config::{AccountConfig, AccountKind, DAY_SECS, SyncSettings};
use crate::ops::{AccountSyncStatus, Secret};
use crate::provider::imap_smtp::ImapSmtpProvider;
use crate::store::Store;
use crate::test_support::sample_account;

const ADDRESS: &str = "test@example.com";
const NOW: i64 = 20_000 * DAY_SECS;

struct Harness {
    _tmp: tempfile::TempDir,
    db: PathBuf,
    writer: WriterHandle,
    _task: WriterTask,
    board: StatusBoard,
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("rail.db");
    let store = Store::open(&db).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('test', 'imapsmtp', ?1)",
            [ADDRESS],
        )
        .unwrap();
    let (writer, task) = spawn_writer(&db).unwrap();
    Harness {
        _tmp: tmp,
        db,
        writer,
        _task: task,
        board: StatusBoard::default(),
    }
}

fn imap_account() -> AccountConfig {
    AccountConfig {
        name: "test".to_string(),
        address: ADDRESS.to_string(),
        kind: AccountKind::ImapSmtp,
        ..sample_account()
    }
}

fn settings() -> SyncSettings {
    SyncSettings {
        history_days: 30,
        body_cache_days: 30,
        poll_interval: Duration::from_secs(5),
    }
}

#[tokio::test]
async fn test_imap_provider_initializes_with_sync_engine() {
    let h = harness();
    let account = imap_account();
    let id = Store::open_reader(&h.db)
        .unwrap()
        .account_id(ADDRESS)
        .unwrap()
        .unwrap();

    h.board
        .insert(AccountSyncStatus::new(&account.name, ADDRESS, account.kind));

    let provider = ImapSmtpProvider::new(
        "imap.example.com".into(),
        993,
        crate::config::model::TlsMode::Required,
        ADDRESS.into(),
        Secret::new("pass".to_string()),
        "smtp.example.com".into(),
        Some(465),
        crate::config::model::TlsMode::Required,
        Some(ADDRESS.into()),
        Some(Secret::new("pass".to_string())),
        Some("Sent".into()),
    );

    let engine = Engine::new(EngineParts {
        account,
        account_id: id,
        settings: settings(),
        provider: Arc::new(provider),
        writer: h.writer.clone(),
        reader: Store::open_reader(&h.db).unwrap(),
        board: h.board.clone(),
        wake: Arc::new(Notify::new()),
        outbox: Arc::new(Notify::new()),
        clock: Arc::new(|| NOW),
    });

    assert!(Arc::strong_count(&engine.wake_handle()) >= 1);
}
