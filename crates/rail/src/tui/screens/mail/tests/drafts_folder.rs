//! An account's Drafts folder: local, pushed and Gmail drafts in one pane, Enter resuming or
//! importing, `d` discarding each kind, thread actions refused, the pane read again on a
//! change, and `:drafts` reading the same rows for every account in the unified inbox.

use rail_core::config::Icons;
use rail_core::ops::{DraftImportResponse, DraftSaveResponse, ErrorCode};
use rail_core::provider::{Envelope, FolderKind, Mailbox, RemoteFolder};
use rail_core::store::drafts::{DraftKind, DraftWrite, Recipient};

use super::compose::{calls_of, compose_open, composing, type_text};
use super::*;
use crate::tui::modals::Target;

/// When the seeded drafts were saved: after every fixture message, a minute apart.
pub(super) const LOCAL_AT: i64 = BASE_MS / 1000 + 3_600;

pub(super) fn recipient(name: Option<&str>, address: &str) -> Recipient {
    Recipient {
        name: name.map(str::to_string),
        address: address.into(),
    }
}

/// A local draft of account `account_id` saved at `at`; it must get `id`.
pub(super) fn seed_local(
    f: &Fixture,
    id: i64,
    account_id: i64,
    subject: &str,
    to: Recipient,
    at: i64,
) {
    let write = DraftWrite {
        id: None,
        account_id,
        kind: DraftKind::New,
        thread_id: None,
        remote_thread_id: None,
        in_reply_to: None,
        references: None,
        to: vec![to],
        cc: vec![],
        bcc: vec![],
        subject: subject.into(),
        body: format!("body of {subject}\n"),
        attachments: vec![],
        message_id: format!("<d{id}@example.com>"),
        source_remote_id: None,
    };
    let made = writer(f).draft_upsert(&write, at).unwrap();
    assert_eq!(made, Some(id), "drafts are numbered in order");
}

/// A synced Gmail draft of `work` in the Drafts folder only.
pub(super) fn gmail_draft(remote: &str, subject: &str, to: &str, secs: i64) -> Envelope {
    Envelope {
        is_draft: true,
        subject: subject.into(),
        to: vec![Mailbox {
            name: Some("Gail".into()),
            address: to.into(),
        }],
        folders: vec!["DRAFT".into()],
        ..mail_envelope(remote, &format!("t-{remote}"), secs * 1000)
    }
}

/// `work`'s folders with Gmail's Drafts added, and its Gmail drafts.
pub(super) fn sync_drafts(f: &Fixture, upserts: Vec<Envelope>) {
    let folders = vec![
        ("INBOX", "Inbox", FolderKind::Inbox),
        ("Label_1", "Projects", FolderKind::User),
        ("SENT", "Sent", FolderKind::Sent),
        ("DRAFT", "Drafts", FolderKind::Drafts),
    ]
    .into_iter()
    .map(|(remote_id, name, kind)| RemoteFolder {
        parent: None,
        remote_id: remote_id.into(),
        name: name.into(),
        kind,
    })
    .collect();
    let batch = Batch {
        folders: Some(folders),
        upserts,
        ..Batch::new(1)
    };
    writer(f).apply(&batch, BASE_MS / 1000).unwrap();
}

fn message_id(f: &Fixture, remote: &str) -> i64 {
    writer(f)
        .conn()
        .query_row(
            "SELECT id FROM messages WHERE remote_id = ?1",
            [remote],
            |r| r.get(0),
        )
        .unwrap()
}

/// `fixture(3)` whose `work` has a Drafts folder holding, newest first: draft 2 (pushed, its
/// Gmail copy `dm1`), the Gmail draft `dm2` no local draft owns, and draft 1 (local only).
pub(super) async fn drafts_fixture() -> Fixture {
    let mut f = fixture(3).await;
    seed_local(
        &f,
        1,
        1,
        "Local plan",
        recipient(None, "lee@example.com"),
        LOCAL_AT,
    );
    seed_local(
        &f,
        2,
        1,
        "Pushed note",
        recipient(Some("Pat Doe"), "pat@example.com"),
        LOCAL_AT + 120,
    );
    writer(&f).draft_set_remote(2, "r1", "dm1").unwrap();
    sync_drafts(
        &f,
        vec![
            gmail_draft("dm1", "Pushed note", "pat@example.com", LOCAL_AT + 120),
            gmail_draft(
                "dm2",
                "Written on the phone",
                "gail@example.com",
                LOCAL_AT + 60,
            ),
        ],
    );
    store_changed(&mut f).await;
    f
}

/// The sidebar's index of `work`'s Drafts folder.
fn drafts_item(f: &Fixture) -> usize {
    f.screen
        .sidebar
        .rows()
        .iter()
        .position(|i| i.label == "Drafts" && i.account() == Some(0))
        .expect("work's Drafts folder is in the sidebar")
}

/// Focus the sidebar and walk its cursor to the Drafts folder; Enter is left to the caller.
async fn point_at_drafts(f: &mut Fixture) {
    press(f, KeyCode::Tab).await;
    let target = drafts_item(f);
    while f.screen.sidebar.cursor() < target {
        press(f, KeyCode::Char('j')).await;
    }
}

pub(super) async fn open_drafts(f: &mut Fixture) {
    point_at_drafts(f).await;
    press(f, KeyCode::Enter).await;
    assert!(f.screen.in_drafts());
}

pub(super) fn pane_subjects(f: &Fixture) -> Vec<String> {
    f.screen
        .drafts
        .pane
        .rows()
        .iter()
        .map(|r| r.subject.clone())
        .collect()
}

fn imports(f: &Fixture) -> Vec<(String, i64)> {
    calls_of(f, |c| matches!(c, FakeCall::DraftImport { .. }))
        .into_iter()
        .filter_map(|c| match c {
            FakeCall::DraftImport {
                account, message, ..
            } => Some((account, message)),
            _ => None,
        })
        .collect()
}

fn discards(f: &Fixture) -> Vec<i64> {
    calls_of(f, |c| matches!(c, FakeCall::DraftDiscard { .. }))
        .into_iter()
        .filter_map(|c| match c {
            FakeCall::DraftDiscard { draft } => Some(draft),
            _ => None,
        })
        .collect()
}

fn drawn(f: &Fixture, width: u16, height: u16) -> String {
    crate::tui::test_util::render_lines(width, height, |frame| {
        f.screen.view(frame, &Theme::default());
    })
    .join("\n")
}

/// Run `effects` one job at a time, returning every event they report, without feeding them.
async fn events_of(effects: Vec<Effect>) -> Vec<TaskResult> {
    let mut all = Vec::new();
    for effect in effects {
        all.extend(run_job(effect).await);
    }
    all
}

#[tokio::test]
async fn the_drafts_folder_lists_local_pushed_and_gmail_rows() {
    let mut f = drafts_fixture().await;
    point_at_drafts(&mut f).await;
    let effects = f.screen.update(Msg::Key(key(KeyCode::Enter)));
    let reported = events_of(effects).await;
    assert!(
        reported
            .iter()
            .any(|r| matches!(r, TaskResult::Mail(MailEvent::DraftsFolder { .. }))),
        "a drafts_folder read goes out: {reported:?}"
    );
    assert!(
        !reported
            .iter()
            .any(|r| matches!(r, TaskResult::Mail(MailEvent::Listed { .. }))),
        "no thread list read: {reported:?}"
    );
    let more = feed(&mut f, reported);
    settle(&mut f.screen, more).await;
    assert!(f.screen.in_drafts());
    assert_eq!(f.screen.focus, Focus::List);
    assert_eq!(
        pane_subjects(&f),
        ["Pushed note", "Written on the phone", "Local plan"]
    );
    let shown = drawn(&f, 150, 30);
    let line = |subject: &str| {
        shown
            .lines()
            .find(|l| l.contains(subject))
            .unwrap_or_else(|| panic!("{subject} is drawn:\n{shown}"))
            .to_string()
    };
    let local = line("Local plan");
    assert!(local.contains("(local)"), "{local}");
    assert!(local.contains("✎ Local plan"), "{local}");
    let pushed = line("Pushed note");
    assert!(pushed.contains("Pat Doe"), "{pushed}");
    assert!(!pushed.contains("(local)"), "{pushed}");
    let gmail = line("Written on the phone");
    assert!(gmail.contains("Gail"), "{gmail}");
    assert!(!gmail.contains("(local)"), "{gmail}");
}

#[tokio::test]
async fn enter_resumes_a_local_draft_and_imports_a_gmail_one() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    press_n(&mut f, KeyCode::Char('j'), 2).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(compose_open(&f), "the review opens on the local draft");
    assert_eq!(composing(&f).draft, Some(1));
    assert!(imports(&f).is_empty(), "a local draft is not imported");
    press(&mut f, KeyCode::Char('q')).await;
    assert!(!compose_open(&f));
    assert!(f.screen.in_drafts(), "back in the folder");
    // The daemon makes the Gmail draft local draft 3 and answers it.
    seed_local(
        &f,
        3,
        1,
        "Written on the phone",
        recipient(Some("Gail"), "gail@example.com"),
        LOCAL_AT + 60,
    );
    writer(&f).draft_set_remote(3, "r2", "dm2").unwrap();
    *f.fake.import_answer.lock().unwrap() = Some(Ok(DraftImportResponse { draft: 3 }));
    press(&mut f, KeyCode::Char('k')).await;
    let effects = f.screen.update(Msg::Key(key(KeyCode::Enter)));
    let line = f.screen.status_line();
    assert!(line.contains("fetching the draft…"), "{line}");
    settle(&mut f.screen, effects).await;
    assert_eq!(imports(&f), [("work".to_string(), message_id(&f, "dm2"))]);
    assert!(compose_open(&f), "the review opens on the imported draft");
    assert_eq!(composing(&f).draft, Some(3));
    assert!(composing(&f).body.contains("Written on the phone"));
}

#[tokio::test]
async fn a_stored_draft_with_stray_carriage_returns_opens_with_plain_line_ends() {
    let mut f = drafts_fixture().await;
    writer(&f)
        .conn()
        .execute("UPDATE drafts SET body = ?1 WHERE id = 1", ["a\r\r\nb\r\n"])
        .unwrap();
    open_drafts(&mut f).await;
    press_n(&mut f, KeyCode::Char('j'), 2).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(composing(&f).draft, Some(1));
    // What the review saves or sends comes from this body, with no editor round in between.
    assert_eq!(composing(&f).body, "a\nb\n");
}

/// A discarded draft cannot be undone, so trash without asking (`D` in the vim preset) still
/// asks in Drafts.
#[tokio::test]
async fn trash_without_asking_still_asks_before_discarding_a_draft() {
    let mut f = drafts_fixture().await;
    let text = "[general]\nkeymap = \"vim\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    f.screen.ctx.keymap =
        Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap());
    open_drafts(&mut f).await;
    press(&mut f, KeyCode::Char('D')).await;
    assert!(matches!(f.screen.modal, Some(Modal::Confirm(_))));
    assert!(discards(&f).is_empty(), "asks first");
}

#[tokio::test]
async fn trash_asks_then_discards_each_kind() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    press_n(&mut f, KeyCode::Char('j'), 2).await;
    press(&mut f, KeyCode::Char('d')).await;
    assert!(matches!(f.screen.modal, Some(Modal::Confirm(_))));
    let shown = drawn(&f, 90, 24);
    assert!(shown.contains("Discard this draft? (y/n)"), "{shown}");
    assert!(discards(&f).is_empty(), "asks first");
    // The daemon deletes it, as `draft.discard` would.
    writer(&f).draft_delete(1).unwrap();
    press(&mut f, KeyCode::Char('y')).await;
    assert_eq!(discards(&f), [1]);
    assert!(f.screen.status_line().contains("draft discarded"));
    assert_eq!(
        pane_subjects(&f),
        ["Pushed note", "Written on the phone"],
        "the pane is read again"
    );
    // The Gmail row: imported first, then the answered draft discarded.
    *f.fake.import_answer.lock().unwrap() = Some(Ok(DraftImportResponse { draft: 3 }));
    press(&mut f, KeyCode::Home).await;
    press(&mut f, KeyCode::Char('j')).await;
    press(&mut f, KeyCode::Char('d')).await;
    let shown = drawn(&f, 90, 24);
    assert!(shown.contains("Discard this draft? (y/n)"), "{shown}");
    assert!(
        shown.contains("Gmail"),
        "says Gmail's copy goes too:\n{shown}"
    );
    press(&mut f, KeyCode::Char('y')).await;
    assert_eq!(imports(&f), [("work".to_string(), message_id(&f, "dm2"))]);
    assert_eq!(discards(&f), [1, 3], "the imported draft is discarded");
    assert!(!compose_open(&f), "nothing opens");
    // The pushed row while its push is in flight: the refusal is on the bar.
    *f.fake.discard_answer.lock().unwrap() = Some(Err(OpError::invalid_input(
        "draft: saving to Gmail, try again in a moment",
    )));
    press(&mut f, KeyCode::Home).await;
    press(&mut f, KeyCode::Char('d')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert_eq!(discards(&f), [1, 3, 2]);
    let line = f.screen.status_line();
    assert!(
        line.contains("draft: saving to Gmail, try again in a moment"),
        "{line}"
    );
    // `n` keeps it.
    press(&mut f, KeyCode::Char('d')).await;
    press(&mut f, KeyCode::Char('n')).await;
    assert_eq!(discards(&f), [1, 3, 2]);
}

#[tokio::test]
async fn thread_actions_are_not_available_on_drafts() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    let before = f.fake.calls.lock().unwrap().len();
    for c in ['e', 's', 'u', 'l', 'm', ' ', 'r', 'R', 'f', '/'] {
        f.screen.status.notify("", f.screen.tick);
        press(&mut f, KeyCode::Char(c)).await;
        let line = f.screen.status_line();
        assert!(line.contains("Not available on drafts"), "{c:?}: {line}");
        assert!(f.screen.modal.is_none(), "{c:?} opens nothing");
        assert!(!compose_open(&f), "{c:?} opens nothing");
    }
    assert!(f.screen.actions.selection.is_empty());
    f.screen.status.notify("", f.screen.tick);
    press(&mut f, KeyCode::Esc).await;
    let line = f.screen.status_line();
    assert!(
        !line.contains("Not available on drafts"),
        "Esc is silent: {line}"
    );
    assert_eq!(f.fake.calls.lock().unwrap().len(), before, "no call");
    assert_eq!(pane_subjects(&f).len(), 3);
}

/// `* a`, `* f` and `v` are refused here like `Space` and `/`: the pane draws drafts, and the
/// threads a selection would reach are the ones the folder replaced, which are drawn nowhere.
#[tokio::test]
async fn bulk_selection_keys_are_not_available_on_drafts() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    let before = f.fake.calls.lock().unwrap().len();
    let reads = f.screen.ctx.store.queries();
    for keys in [['*', 'a'], ['*', 'f'], ['v', 'v']] {
        f.screen.status.notify("", f.screen.tick);
        for c in keys {
            press(&mut f, KeyCode::Char(c)).await;
        }
        let line = f.screen.status_line();
        assert!(line.contains("Not available on drafts"), "{keys:?}: {line}");
        assert!(f.screen.modal.is_none(), "{keys:?} opens nothing");
        assert!(
            f.screen.actions.selection.is_empty(),
            "{keys:?} selects nothing"
        );
        assert!(
            f.screen.filter_text.is_empty(),
            "{keys:?} leaves the filter alone: {:?}",
            f.screen.filter_text
        );
        assert!(!f.screen.bulk_range_on(), "{keys:?} arms no range");
    }
    assert_eq!(f.screen.ctx.store.queries(), reads, "no read");
    assert_eq!(f.fake.calls.lock().unwrap().len(), before, "no call");
    assert_eq!(pane_subjects(&f).len(), 3);
}

#[tokio::test]
async fn a_failed_import_says_so_and_stays_in_the_folder() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    *f.fake.import_answer.lock().unwrap() = Some(Err(OpError::new(
        ErrorCode::NotFound,
        rail_core::error::ErrorClass::Permanent,
        "Gmail no longer has this draft",
    )));
    press(&mut f, KeyCode::Char('j')).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(imports(&f).len(), 1);
    let line = f.screen.status_line();
    assert!(line.contains("Gmail no longer has this draft"), "{line}");
    assert!(!compose_open(&f));
    assert!(f.screen.in_drafts());
    assert_eq!(pane_subjects(&f).len(), 3, "the pane stays");
    assert_eq!(f.screen.drafts.pane.cursor(), 1);
    // Another Enter tries again: nothing is left pending.
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(imports(&f).len(), 2);
}

#[tokio::test]
async fn a_pane_enter_while_an_import_is_pending_says_busy() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    press(&mut f, KeyCode::Char('j')).await;
    let pending = f.screen.update(Msg::Key(key(KeyCode::Enter)));
    assert_eq!(pending.len(), 1, "the import call");
    for code in [KeyCode::Enter, KeyCode::Char('d')] {
        let effects = f.screen.update(Msg::Key(key(code)));
        if code == KeyCode::Char('d') {
            let more = f.screen.update(Msg::Key(key(KeyCode::Char('y'))));
            assert!(more.is_empty(), "no discard while importing");
        }
        assert!(effects.is_empty(), "{code:?} starts nothing");
        let line = f.screen.status_line();
        assert!(line.contains(composing::BUSY), "{line}");
    }
    *f.fake.import_answer.lock().unwrap() = Some(Ok(DraftImportResponse { draft: 2 }));
    settle(&mut f.screen, pending).await;
    assert!(compose_open(&f), "the import's answer still opens it");
    assert_eq!(composing(&f).draft, Some(2));
}

#[tokio::test]
async fn colon_drafts_lists_every_account_in_the_unified_inbox() {
    let mut f = drafts_fixture().await;
    seed_local(
        &f,
        3,
        2,
        "Home errand",
        recipient(None, "kim@example.com"),
        LOCAL_AT + 600,
    );
    assert_eq!(f.screen.scope, ScopeChoice::Unified);
    let effects = f.screen.run_palette(Target::Drafts);
    let reported = events_of(effects).await;
    assert!(
        reported
            .iter()
            .any(|r| matches!(r, TaskResult::Mail(MailEvent::DraftsFolder { .. }))),
        "{reported:?}"
    );
    let more = feed(&mut f, reported);
    settle(&mut f.screen, more).await;
    assert!(matches!(f.screen.modal, Some(Modal::Picker(_))));
    let shown = drawn(&f, 150, 30);
    for subject in [
        "Home errand",
        "Pushed note",
        "Written on the phone",
        "Local plan",
    ] {
        assert!(shown.contains(subject), "{subject}:\n{shown}");
    }
    // The second row is the pushed draft; the third, Gmail's, imports.
    press(&mut f, KeyCode::Down).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(compose_open(&f));
    assert_eq!(composing(&f).draft, Some(2));
    press(&mut f, KeyCode::Char('q')).await;
    let effects = f.screen.run_palette(Target::Drafts);
    settle(&mut f.screen, effects).await;
    *f.fake.import_answer.lock().unwrap() = Some(Ok(DraftImportResponse { draft: 1 }));
    press_n(&mut f, KeyCode::Down, 2).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(imports(&f), [("work".to_string(), message_id(&f, "dm2"))]);
    assert_eq!(composing(&f).draft, Some(1));
    // In an account's scope, only that account's drafts.
    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Char('2')).await;
    let effects = f.screen.run_palette(Target::Drafts);
    settle(&mut f.screen, effects).await;
    let shown = drawn(&f, 150, 30);
    assert!(shown.contains("Home errand"), "{shown}");
    assert!(!shown.contains("Local plan"), "{shown}");
}

#[tokio::test]
async fn the_pane_is_read_again_when_the_outbox_or_sync_changes() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    press(&mut f, KeyCode::Char('j')).await;
    seed_local(
        &f,
        3,
        1,
        "Newest",
        recipient(None, "new@example.com"),
        LOCAL_AT + 900,
    );
    store_changed(&mut f).await;
    assert_eq!(
        pane_subjects(&f),
        [
            "Newest",
            "Pushed note",
            "Written on the phone",
            "Local plan"
        ]
    );
    assert_eq!(
        f.screen.drafts.pane.selected().map(|r| r.subject.as_str()),
        Some("Written on the phone"),
        "the cursor stays on its row, one place down"
    );
    // The outbox: a push lands and the Gmail draft becomes the local draft's copy.
    writer(&f).draft_set_remote(3, "r3", "dm2").unwrap();
    let mut saving = status("work", "w@example.com", SyncPhase::Idle);
    saving.outbox.saving = 1;
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(saving)));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    assert_eq!(pane_subjects(&f), ["Newest", "Pushed note", "Local plan"]);
    // Leaving the folder reads the thread list again.
    press(&mut f, KeyCode::Char('0')).await;
    assert!(!f.screen.in_drafts());
    assert_eq!(f.screen.list.rows().len(), 5);
}

#[tokio::test]
async fn snapshot_drafts_folder_90_and_150_columns() {
    for (width, height, icons) in [(90, 24, Icons::Unicode), (150, 40, Icons::Ascii)] {
        let mut f = drafts_fixture().await;
        f.screen.ctx.icons = icons;
        open_drafts(&mut f).await;
        let shot = crate::tui::test_util::snapshot(width, height, |frame| {
            f.screen.view(frame, &Theme::default())
        });
        insta::assert_snapshot!(format!("drafts_folder_{width}"), shot);
    }
}

/// Each `draft.save` written to the store as the daemon would, the request's subject and
/// recipients kept.
fn store_saves(f: &Fixture) {
    let db = f.paths.db_file();
    *f.fake.on_save.lock().unwrap() = Some(Box::new(move |call| {
        let r = &call.request;
        let write = DraftWrite {
            id: r.draft,
            account_id: 1,
            kind: r.kind,
            thread_id: None,
            remote_thread_id: None,
            in_reply_to: None,
            references: None,
            to: r.to.iter().map(|a| recipient(None, a)).collect(),
            cc: vec![],
            bcc: vec![],
            subject: r.subject.clone(),
            body: r.body.clone(),
            attachments: vec![],
            message_id: format!("<{}@example.com>", r.client_token),
            source_remote_id: None,
        };
        let mut store = rail_core::store::Store::open(&db).unwrap();
        let draft = store
            .draft_upsert(&write, LOCAL_AT + 1_800)
            .unwrap()
            .unwrap();
        Ok(DraftSaveResponse {
            draft,
            message_id: write.message_id.clone(),
            attachments: vec![],
        })
    }));
}

#[tokio::test]
async fn a_save_in_the_folder_reads_the_pane_again() {
    let mut f = drafts_fixture().await;
    store_saves(&f);
    open_drafts(&mut f).await;
    // A new message written from the folder.
    press(&mut f, KeyCode::Char('c')).await;
    type_text(&mut f, "zed@example.com").await;
    press(&mut f, KeyCode::Enter).await;
    type_text(&mut f, "Brand new").await;
    press(&mut f, KeyCode::Enter).await;
    assert!(compose_open(&f));
    press(&mut f, KeyCode::Char('q')).await;
    assert!(!compose_open(&f));
    assert_eq!(pane_subjects(&f)[0], "Brand new", "{:?}", pane_subjects(&f));
    // A local draft resumed and its subject changed in the editor.
    f.editor.set(|path| {
        let text = std::fs::read_to_string(path).unwrap();
        std::fs::write(
            path,
            text.replace("Subject: Local plan", "Subject: Renamed plan"),
        )
        .unwrap();
        Ok(0)
    });
    press(&mut f, KeyCode::End).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(composing(&f).draft, Some(1));
    press(&mut f, KeyCode::Char('e')).await;
    assert_eq!(composing(&f).headers.subject, "Renamed plan");
    press(&mut f, KeyCode::Char('q')).await;
    let subjects = pane_subjects(&f);
    assert!(
        subjects.contains(&"Renamed plan".to_string()),
        "{subjects:?}"
    );
    assert!(
        !subjects.contains(&"Local plan".to_string()),
        "{subjects:?}"
    );
}

#[tokio::test]
async fn entering_the_folder_closes_the_reader_of_the_old_scope() {
    let mut f = drafts_fixture().await;
    let _ = drawn(&f, 150, 30); // the split layout
    press(&mut f, KeyCode::Enter).await;
    assert!(f.screen.reader.is_some(), "a thread is open in the pane");
    let scope = f.screen.sidebar.rows()[drafts_item(&f)]
        .scope()
        .expect("a folder row opens a scope");
    let effects = f.screen.set_scope(scope);
    settle(&mut f.screen, effects).await;
    assert!(f.screen.in_drafts());
    assert!(
        f.screen.reader.is_none(),
        "the old scope's thread is closed"
    );
    press(&mut f, KeyCode::Char('c')).await;
    press(&mut f, KeyCode::Esc).await;
    assert!(!compose_open(&f));
    assert_eq!(
        f.screen.focus,
        Focus::List,
        "back to the pane, not a reader"
    );
    let before = f.fake.calls.lock().unwrap().len();
    press(&mut f, KeyCode::Char('e')).await;
    assert!(f.screen.status_line().contains("Not available on drafts"));
    assert_eq!(
        f.fake.calls.lock().unwrap().len(),
        before,
        "no thread acted on"
    );
}

#[tokio::test]
async fn the_bar_shows_no_filter_in_the_folder() {
    let mut f = drafts_fixture().await;
    f.screen.filter = rail_core::store::filter::parse("from:sender");
    assert!(f.screen.slot_text().contains("from:"));
    open_drafts(&mut f).await;
    let slot = f.screen.slot_text();
    assert_eq!(slot, "", "the folder has no search slot");
    press(&mut f, KeyCode::Char('0')).await;
    assert!(
        f.screen.slot_text().contains("from:"),
        "back in a thread list"
    );
}

#[tokio::test]
async fn a_reply_key_does_not_replace_a_pending_import() {
    let mut f = drafts_fixture().await;
    open_drafts(&mut f).await;
    press(&mut f, KeyCode::Char('j')).await;
    let pending = f.screen.update(Msg::Key(key(KeyCode::Enter)));
    press(&mut f, KeyCode::Char('0')).await;
    assert!(!f.screen.in_drafts());
    for c in ['r', 'R', 'f'] {
        let effects = f.screen.update(Msg::Key(key(KeyCode::Char(c))));
        assert!(effects.is_empty(), "{c:?} starts nothing");
        assert!(
            matches!(
                f.screen.composing.pending,
                Some(composing::Pending::Importing { .. })
            ),
            "{c:?} keeps the import pending"
        );
        assert!(f.screen.status_line().contains(composing::BUSY));
    }
    *f.fake.import_answer.lock().unwrap() = Some(Ok(DraftImportResponse { draft: 2 }));
    settle(&mut f.screen, pending).await;
    assert!(compose_open(&f), "the import's answer still opens it");
}
