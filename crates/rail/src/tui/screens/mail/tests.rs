//! The mail screen's `update` with synthetic keys against a seeded store and a scripted
//! daemon: scopes, paging at both edges, re-finding the cursor, the sidebar, the status bar,
//! refresh and reconnect, and an override in the keymap.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crossterm::event::KeyCode;
use rail_core::ops::{OpError, SyncPhase};
use rail_core::paths::Paths;
use rail_core::store::Store;
use rail_core::store::view::PAGE;
use rail_core::sync::Batch;

use super::*;
use crate::tui::app::{Effect, Msg, TaskResult, TaskSink};
use crate::tui::data::{DaemonEvent, FakeCall, FakeDaemon, MailEvent, Timer};
use crate::tui::keys::Keymap;
use crate::tui::screen::Screen;
use crate::tui::test_util::{
    BASE_MS, Editor, inbox_threads, key, mail_ctx, mail_envelope, seed_store, settle,
    settle_capturing, status, work_and_home,
};
use crate::tui::theme::choice::{ColorDepth, ThemeChoice};

mod actions;
mod attachments;
mod bulk;
mod compose;
mod compose_edges;
mod compose_gmail;
mod dim_read;
mod drafts_folder;
mod keymap;
mod list;
mod m365;
mod pickers;
mod replaced;
mod saved;
mod sidebar;
mod snooze;
mod state;
mod themes;
mod unified;

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: Paths,
    fake: Arc<FakeDaemon>,
    screen: MailScreen,
    /// The last theme an effect applied.
    last_theme: Arc<Mutex<Option<Theme>>>,
    /// What a fake editor does when the screen asks for one.
    editor: Editor,
}

/// `work` with `n` inbox threads and `home` with two, the screen started and settled.
async fn fixture(n: usize) -> Fixture {
    fixture_with_depth(n, ColorDepth::TrueColor).await
}

/// `fixture` on a terminal of `depth`.
async fn fixture_with_depth(n: usize, depth: ColorDepth) -> Fixture {
    fixture_with(n, depth, ThemeChoice::default()).await
}

/// `fixture` on a terminal of `depth` with `choice` configured.
async fn fixture_with(n: usize, depth: ColorDepth, choice: ThemeChoice) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    let mut envelopes: Vec<_> = inbox_threads("w", n).into_iter().map(|e| (0, e)).collect();
    envelopes.extend(inbox_threads("h", 2).into_iter().map(|e| (1, e)));
    seed_store(
        &paths,
        &[("work", "w@example.com"), ("home", "h@example.com")],
        &envelopes,
    );
    let fake = Arc::new(FakeDaemon::default());
    *fake.statuses.lock().unwrap() = vec![
        status("work", "w@example.com", SyncPhase::Idle),
        status("home", "h@example.com", SyncPhase::Idle),
    ];
    let ctx = MailContext {
        depth,
        theme_choice: choice,
        ..mail_ctx(&paths, fake.clone())
    };
    let mut screen = MailScreen::new(ctx, work_and_home());
    let last_theme = Arc::new(Mutex::new(None));
    let editor = Editor::default();
    let effects = screen.start();
    settle_capturing(&mut screen, effects, &last_theme, &editor).await;
    Fixture {
        _tmp: tmp,
        paths,
        fake,
        screen,
        last_theme,
        editor,
    }
}

async fn press(f: &mut Fixture, code: KeyCode) {
    let effects = f.screen.update(Msg::Key(key(code)));
    settle_capturing(&mut f.screen, effects, &f.last_theme, &f.editor).await;
}

/// Tick until the notice has expired, so the hint bar shows the hints again.
async fn expire_notice(f: &mut Fixture) {
    for _ in 0..=status::NOTICE_TICKS {
        let effects = f.screen.update(Msg::Tick);
        settle(&mut f.screen, effects).await;
    }
}

async fn press_n(f: &mut Fixture, code: KeyCode, times: usize) {
    for _ in 0..times {
        press(f, code).await;
    }
}

/// The `Apply` calls the fake daemon has taken.
fn applies(f: &Fixture) -> Vec<FakeCall> {
    f.fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::Apply { .. }))
        .cloned()
        .collect()
}

fn selected(f: &Fixture) -> Option<String> {
    f.screen.list.selected().map(|r| r.subject.clone())
}

fn thread_ids(f: &Fixture) -> Vec<i64> {
    f.screen
        .list
        .rows()
        .iter()
        .map(|r| r.key.thread_id)
        .collect()
}

/// Feed a store change as the daemon would and let the debounce run.
async fn store_changed(f: &mut Fixture) {
    let status = status("work", "w@example.com", SyncPhase::Delta);
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status)));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
}

fn writer(f: &Fixture) -> Store {
    Store::open(&f.paths.db_file()).unwrap()
}

#[tokio::test]
async fn start_reads_the_unified_inbox_the_sidebar_and_subscribes() {
    let f = fixture(3).await;
    assert_eq!(f.screen.list.rows().len(), 5, "both accounts' inboxes");
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm2"),
        "the newest first"
    );
    assert_eq!(*f.fake.calls.lock().unwrap(), [FakeCall::Watch]);
    assert_eq!(f.screen.status.daemon, DaemonState::Connected);
    assert_eq!(f.screen.store_state, StoreState::Ready);
}

#[tokio::test]
async fn number_keys_and_zero_change_the_scope() {
    let mut f = fixture(3).await;
    press(&mut f, KeyCode::Char('2')).await;
    assert_eq!(f.screen.scope, ScopeChoice::Account(1));
    assert_eq!(f.screen.list.rows().len(), 2);
    press(&mut f, KeyCode::Char('1')).await;
    assert_eq!(f.screen.list.rows().len(), 3);
    press(&mut f, KeyCode::Char('9')).await;
    assert_eq!(
        f.screen.scope,
        ScopeChoice::Account(0),
        "no ninth account: unchanged"
    );
    assert!(f.screen.status.notice().unwrap().contains("no account 9"));
    press(&mut f, KeyCode::Char('0')).await;
    assert_eq!(f.screen.scope, ScopeChoice::Unified);
    assert_eq!(f.screen.list.rows().len(), 5);
}

#[tokio::test]
async fn keyset_paging_reaches_both_edges_and_keeps_the_window_bounded() {
    let mut f = fixture(700).await;
    press(&mut f, KeyCode::Char('1')).await;
    assert_eq!(f.screen.list.rows().len(), PAGE);
    press_n(&mut f, KeyCode::Char('j'), 450).await;
    assert_eq!(selected(&f).as_deref(), Some("subject wm249"));
    assert!(f.screen.list.rows().len() <= MAX_WINDOW);
    press(&mut f, KeyCode::End).await;
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm0"),
        "the oldest thread"
    );
    press_n(&mut f, KeyCode::Char('k'), 350).await;
    assert_eq!(selected(&f).as_deref(), Some("subject wm350"));
    press(&mut f, KeyCode::Home).await;
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm699"),
        "the newest thread"
    );
    let ids = thread_ids(&f);
    assert_eq!(
        ids.len(),
        ids.iter().collect::<BTreeSet<_>>().len(),
        "no thread twice"
    );
}

/// Review focus 1.
#[tokio::test]
async fn a_vanished_selection_keeps_its_index_and_an_emptied_list_is_safe() {
    let mut f = fixture(5).await;
    press(&mut f, KeyCode::Char('1')).await;
    press_n(&mut f, KeyCode::Char('j'), 2).await;
    assert_eq!(selected(&f).as_deref(), Some("subject wm2"));
    let mut store = writer(&f);
    store
        .apply(
            &Batch {
                deletes: vec!["wm2".into()],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    store_changed(&mut f).await;
    assert_eq!(f.screen.list.selected_index(), 2, "the same place");
    assert_eq!(selected(&f).as_deref(), Some("subject wm1"));
    press(&mut f, KeyCode::End).await;
    let deletes: Vec<String> = ["wm0", "wm1", "wm3", "wm4"].map(String::from).to_vec();
    store
        .apply(
            &Batch {
                deletes,
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    store_changed(&mut f).await;
    assert!(f.screen.list.rows().is_empty());
    assert_eq!(f.screen.list.selected(), None);
    for code in [
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::End,
        KeyCode::Home,
        KeyCode::Enter,
    ] {
        press(&mut f, code).await;
    }
    let text = crate::tui::test_util::snapshot(90, 24, |frame| {
        f.screen.view(frame, &crate::tui::theme::Theme::default())
    });
    assert!(text.contains("No threads here."), "{text}");
}

#[tokio::test]
async fn the_cursor_follows_its_thread_when_new_mail_arrives_above() {
    let mut f = fixture(5).await;
    press(&mut f, KeyCode::Char('1')).await;
    press(&mut f, KeyCode::Char('j')).await;
    assert_eq!(selected(&f).as_deref(), Some("subject wm3"));
    let mut store = writer(&f);
    let newer = mail_envelope("wnew", "twnew", BASE_MS + 99 * 60_000);
    store
        .apply(
            &Batch {
                upserts: vec![newer],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    store_changed(&mut f).await;
    assert_eq!(f.screen.list.rows()[0].subject, "subject wnew");
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm3"),
        "re-found by id"
    );
}

/// Review focus 2: a thread whose newest message is deleted sorts much older; the page that
/// brings it must not leave its stale copy in the window.
#[tokio::test]
async fn a_thread_that_moves_while_paging_is_shown_once() {
    let mut f = fixture(250).await;
    let mut store = writer(&f);
    let mut old = mail_envelope("wold", "wt240", BASE_MS - 86_400_000);
    // The thread's subject is its earliest message's: keep it the same.
    old.subject = "subject wm240".into();
    store
        .apply(
            &Batch {
                upserts: vec![old],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    press(&mut f, KeyCode::Char('1')).await;
    assert!(
        f.screen
            .list
            .rows()
            .iter()
            .any(|r| r.subject.contains("wm240"))
    );
    store
        .apply(
            &Batch {
                deletes: vec!["wm240".into()],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    press_n(&mut f, KeyCode::Char('j'), 260).await;
    press(&mut f, KeyCode::End).await;
    let ids = thread_ids(&f);
    assert_eq!(
        ids.len(),
        ids.iter().collect::<BTreeSet<_>>().len(),
        "{ids:?}"
    );
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm240"),
        "the oldest now, once"
    );
}

#[tokio::test]
async fn a_requery_after_scrolling_past_a_page_keeps_the_same_thread() {
    let mut f = fixture(700).await;
    press(&mut f, KeyCode::Char('1')).await;
    press_n(&mut f, KeyCode::Char('j'), 150).await;
    assert_eq!(selected(&f).as_deref(), Some("subject wm549"));
    store_changed(&mut f).await;
    assert_eq!(
        selected(&f).as_deref(),
        Some("subject wm549"),
        "the same thread"
    );
    assert!(f.screen.list.rows().len() <= MAX_WINDOW);
}

#[tokio::test]
async fn a_requery_during_a_scope_change_does_not_override_it() {
    let mut f = fixture(3).await;
    press(&mut f, KeyCode::Char('j')).await;
    assert_eq!(f.screen.list.selected_index(), 1);
    let switch = f.screen.update(Msg::Key(key(KeyCode::Char('2'))));
    let progress = status("work", "w@example.com", SyncPhase::Delta);
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(progress)));
    let timer = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, timer).await;
    settle(&mut f.screen, switch).await;
    assert_eq!(f.screen.scope, ScopeChoice::Account(1));
    assert_eq!(f.screen.list.rows().len(), 2, "the new scope's rows");
    assert_eq!(f.screen.list.selected_index(), 0, "at the top");
}

#[tokio::test]
async fn a_requery_during_end_does_not_drop_it() {
    let mut f = fixture(5).await;
    press(&mut f, KeyCode::Char('1')).await;
    let end = f.screen.update(Msg::Key(key(KeyCode::End)));
    let progress = status("work", "w@example.com", SyncPhase::Delta);
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(progress)));
    let timer = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, timer).await;
    settle(&mut f.screen, end).await;
    assert!(f.screen.list.rows().len() <= 5);
}

#[tokio::test]
async fn store_changes_are_coalesced_and_an_idle_screen_runs_nothing() {
    let mut f = fixture(3).await;
    let progress = |f: &mut Fixture| {
        let status = status("work", "w@example.com", SyncPhase::Backfill);
        let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status)));
        f.screen.update(Msg::Task(TaskResult::Mail(event)))
    };
    let first = progress(&mut f);
    assert_eq!(first.len(), 1, "one timer: {first:?}");
    assert!(progress(&mut f).is_empty(), "folded into the running timer");
    assert!(progress(&mut f).is_empty());
    settle(&mut f.screen, first).await;
    for _ in 0..20 {
        assert!(f.screen.update(Msg::Tick).is_empty(), "ticks read nothing");
    }
    let stale = MailEvent::Timer {
        token: 0,
        kind: Timer::Requery,
    };
    assert!(
        f.screen
            .update(Msg::Task(TaskResult::Mail(stale)))
            .is_empty()
    );
}

/// Run one job to its end and collect what it reports.
async fn run_job(effect: Effect) -> Vec<TaskResult> {
    let Effect::Spawn(job) = effect else {
        panic!("expected a job, got {effect:?}");
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        job(TaskSink::new(tx, 0)),
    )
    .await
    .expect("a job finished in time");
    let mut results = Vec::new();
    while let Ok((_, result)) = rx.try_recv() {
        results.push(result);
    }
    results
}

fn feed(f: &mut Fixture, results: Vec<TaskResult>) -> Vec<Effect> {
    results
        .into_iter()
        .flat_map(|r| f.screen.update(Msg::Task(r)))
        .collect()
}

#[tokio::test]
async fn one_refresh_runs_at_a_time_and_changes_meanwhile_run_it_once_more() {
    let mut f = fixture(3).await;
    let progress = |f: &mut Fixture| {
        let status = status("work", "w@example.com", SyncPhase::Backfill);
        let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status)));
        f.screen.update(Msg::Task(TaskResult::Mail(event)))
    };
    let mut timer = progress(&mut f);
    assert_eq!(timer.len(), 1, "{timer:?}");
    let fired = run_job(timer.remove(0)).await;
    let mut refresh = feed(&mut f, fired);
    assert_eq!(refresh.len(), 1, "one refresh job: {refresh:?}");
    // The refresh is in flight: further changes start nothing.
    assert!(progress(&mut f).is_empty());
    assert!(progress(&mut f).is_empty());
    let answers = run_job(refresh.remove(0)).await;
    let next = feed(&mut f, answers);
    assert_eq!(
        next.len(),
        1,
        "one timer for the changes meanwhile: {next:?}"
    );
    let queries = f.screen.ctx.store.queries();
    settle(&mut f.screen, next).await;
    assert_eq!(
        f.screen.ctx.store.queries() - queries,
        2,
        "exactly one more refresh: its list and its sidebar"
    );
    assert!(f.screen.refreshing.is_none() && !f.screen.dirty_again);
    assert_eq!(
        progress(&mut f).len(),
        1,
        "an idle screen arms the timer again"
    );
}

#[tokio::test]
async fn the_sidebar_toggles_focuses_and_opens_a_label() {
    let mut f = fixture(3).await;
    let mut store = writer(&f);
    let mut labelled = mail_envelope("wl", "twl", BASE_MS - 60_000);
    labelled.folders = vec!["Label_1".into()];
    store
        .apply(
            &Batch {
                upserts: vec![labelled],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    store_changed(&mut f).await;
    assert!(
        !f.screen.sidebar_visible(),
        "hidden before a frame says the width"
    );
    press(&mut f, KeyCode::Tab).await;
    assert!(f.screen.sidebar_visible());
    assert_eq!(f.screen.focus, Focus::Sidebar);
    let labels: Vec<String> = f
        .screen
        .sidebar
        .rows()
        .iter()
        .map(|r| r.label.clone())
        .collect();
    assert_eq!(
        labels,
        [
            "UNIFIED",
            "All inboxes",
            "Starred",
            "Drafts",
            "Sent",
            "Trash",
            "",
            "ACCOUNTS",
            "work",
            "Inbox",
            "Sent",
            "",
            "More folders (1)",
            "home",
            "Inbox",
            "Sent",
            "",
            "More folders (1)"
        ]
    );
    sidebar::open_projects(&mut f).await;
    assert!(
        matches!(&f.screen.scope, ScopeChoice::Folder { account: 0, name, .. } if name == "Projects")
    );
    assert_eq!(f.screen.focus, Focus::List);
    assert_eq!(selected(&f).as_deref(), Some("subject wl"));
    press(&mut f, KeyCode::Char('b')).await;
    assert!(!f.screen.sidebar_visible());
    press(&mut f, KeyCode::Char('b')).await;
    assert!(f.screen.sidebar_visible());
}

#[tokio::test]
async fn progress_reaches_the_status_bar_and_a_lost_daemon_says_so() {
    let mut f = fixture(1).await;
    let mut backfill = status("home", "h@example.com", SyncPhase::Backfill);
    backfill.backfill_fetched = 1_200;
    backfill.backfill_estimate = Some(5_000);
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(backfill)));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    let line = f.screen.status_line();
    assert!(
        line.contains("work ✓") && line.contains("home ⟳ 1,200/5,000"),
        "{line}"
    );
    let lost = MailEvent::Daemon(DaemonEvent::Lost(OpError::internal("gone")));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(lost)));
    settle(&mut f.screen, effects).await;
    assert_eq!(f.screen.status.daemon, DaemonState::Unreachable);
    assert!(
        f.screen
            .header_summary()
            .contains("daemon unreachable (Ctrl-r retries)")
    );
    assert_eq!(f.screen.status.errors().count(), 1, ":errors keeps it");
}

#[tokio::test]
async fn ctrl_r_refreshes_when_connected_and_reconnects_when_not() {
    let mut f = fixture(1).await;
    let effects = f.screen.update(Msg::Key(crate::tui::test_util::ctrl('r')));
    settle(&mut f.screen, effects).await;
    assert_eq!(
        f.fake.calls.lock().unwrap().last(),
        Some(&FakeCall::Refresh)
    );
    f.fake
        .unreachable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let lost = MailEvent::Daemon(DaemonEvent::Lost(OpError::internal("gone")));
    f.screen.update(Msg::Task(TaskResult::Mail(lost)));
    f.fake
        .unreachable
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let effects = f.screen.update(Msg::Key(crate::tui::test_util::ctrl('r')));
    settle(&mut f.screen, effects).await;
    assert_eq!(
        f.fake.calls.lock().unwrap().last(),
        Some(&FakeCall::Watch),
        "re-subscribed"
    );
    assert_eq!(f.screen.status.daemon, DaemonState::Connected);
}

#[tokio::test]
async fn a_live_watch_is_never_started_twice() {
    let mut f = fixture(1).await;
    let refused = MailEvent::Daemon(DaemonEvent::Refreshed(Err(OpError::internal("busy"))));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(refused)));
    settle(&mut f.screen, effects).await;
    assert_eq!(
        f.screen.status.daemon,
        DaemonState::Connected,
        "a failed sync.now is not a lost link"
    );
    let effects = f.screen.update(Msg::Key(crate::tui::test_util::ctrl('r')));
    settle(&mut f.screen, effects).await;
    let calls = f.fake.calls.lock().unwrap().clone();
    assert_eq!(
        calls.iter().filter(|c| **c == FakeCall::Watch).count(),
        1,
        "{calls:?}"
    );
    // Still connecting: Ctrl-r asks the daemon nothing.
    f.screen.status.connecting();
    let effects = f.screen.update(Msg::Key(crate::tui::test_util::ctrl('r')));
    settle(&mut f.screen, effects).await;
    assert_eq!(f.fake.calls.lock().unwrap().len(), calls.len());
}

#[tokio::test]
async fn a_store_that_is_not_set_up_shows_the_hint_in_the_list() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    let fake = Arc::new(FakeDaemon::default());
    let mut screen = MailScreen::new(mail_ctx(&paths, fake), work_and_home());
    let effects = screen.start();
    settle(&mut screen, effects).await;
    assert_eq!(screen.store_state, StoreState::NotSetUp);
    let text = crate::tui::test_util::snapshot(90, 24, |frame| {
        screen.view(frame, &crate::tui::theme::Theme::default())
    });
    assert!(text.contains("the store is not set up"), "{text}");
    assert!(text.contains("rail sync"), "{text}");
    seed_store(
        &paths,
        &[("work", "w@example.com")],
        &[(0, mail_envelope("m", "t", BASE_MS))],
    );
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status(
        "work",
        "w@example.com",
        SyncPhase::Backfill,
    ))));
    let effects = screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut screen, effects).await;
    assert_eq!(
        screen.store_state,
        StoreState::Ready,
        "it reads once the daemon set it up"
    );
    assert_eq!(screen.list.rows().len(), 1);
}

#[tokio::test]
async fn an_override_in_the_keymap_drives_the_list() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    let envelopes: Vec<_> = inbox_threads("w", 3).into_iter().map(|e| (0, e)).collect();
    seed_store(&paths, &[("work", "w@example.com")], &envelopes);
    let text = "[keys]\n\"list.down\" = \"x\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    let mut ctx = mail_ctx(&paths, Arc::new(FakeDaemon::default()));
    ctx.keymap =
        Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap());
    let mut screen = MailScreen::new(ctx, work_and_home()[..1].to_vec());
    let effects = screen.start();
    settle(&mut screen, effects).await;
    screen.update(Msg::Key(key(KeyCode::Char('j'))));
    assert_eq!(screen.list.selected_index(), 0, "j no longer moves");
    screen.update(Msg::Key(key(KeyCode::Char('x'))));
    assert_eq!(screen.list.selected_index(), 1);
}

#[tokio::test]
async fn q_quits_from_the_list_and_ctrl_c_always() {
    let f = fixture(1).await;
    assert!(
        !f.screen.claims_key(key(KeyCode::Char('q'))),
        "global.quit is the shell's"
    );
    assert!(f.screen.claims_key(key(KeyCode::Char('j'))));
}

#[tokio::test]
async fn a_key_that_ends_no_chord_is_read_on_its_own() {
    let mut f = fixture(3).await;
    let g = f.screen.update(Msg::Key(key(KeyCode::Char('g'))));
    assert!(g.is_empty() && f.screen.chord.is_pending());
    let quit = f.screen.update(Msg::Key(key(KeyCode::Char('q'))));
    assert!(
        matches!(quit.as_slice(), [Effect::Quit]),
        "g then q quits: {quit:?}"
    );
    assert!(!f.screen.chord.is_pending());
    f.screen.update(Msg::Key(key(KeyCode::Char('g'))));
    press(&mut f, KeyCode::Char('j')).await;
    assert_eq!(f.screen.list.selected_index(), 1, "g then j moves");
    assert!(!f.screen.chord.is_pending());
}

#[tokio::test]
async fn a_focus_change_drops_a_waiting_chord_key() {
    let mut f = fixture(3).await;
    let open = f.screen.update(Msg::Key(key(KeyCode::Enter)));
    f.screen.update(Msg::Key(key(KeyCode::Char('g'))));
    assert!(f.screen.chord.is_pending());
    settle(&mut f.screen, open).await;
    assert_eq!(f.screen.focus, Focus::Reader);
    assert!(
        !f.screen.chord.is_pending(),
        "the list's g is not the reader's"
    );
}

#[test]
fn tiny_terminals_draw_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::all_in(tmp.path());
    let screen = MailScreen::new(
        mail_ctx(&paths, Arc::new(FakeDaemon::default())),
        work_and_home(),
    );
    for (width, height) in [
        (1, 1),
        (2, 2),
        (5, 3),
        (20, 3),
        (40, 2),
        (99, 4),
        (100, 4),
        (200, 2),
    ] {
        crate::tui::test_util::render_lines(width, height, |frame| {
            screen.view(frame, &crate::tui::theme::Theme::default())
        });
    }
}
