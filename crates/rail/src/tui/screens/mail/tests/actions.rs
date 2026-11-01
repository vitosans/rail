//! Actions from the list and the reader with the fake daemon over a seeded store: each key,
//! the selection across a re-query and a scope change, the trash prompt, the undo notice and
//! `z` and its expiry, mark-read on open, the status bar depth and the failure line, and
//! `:errors` retry.

use rail_core::ops::{
    Action as MailAction, ActionApplyResponse, ActionFailed, OpError, OutboxDepth,
};
use rail_core::store::Store;
use rail_core::store::outbox::{NewAction, OutboxOutcome, OutboxWrite};

use super::*;
use crate::tui::data::ApplyCall;
use crate::tui::modals::Target;

/// The fake applies each call to the store as the daemon would (rows ids from the store).
fn applying(f: &Fixture) {
    let db = f.paths.db_file();
    *f.fake.on_apply.lock().unwrap() = Some(Box::new(move |call: &ApplyCall| {
        let mut store = Store::open(&db).unwrap();
        let account: i64 = store
            .conn()
            .query_row(
                "SELECT id FROM accounts WHERE name = ?1",
                [&call.account],
                |r| r.get(0),
            )
            .unwrap();
        let new = NewAction {
            account_id: account,
            action: call.action.clone(),
            threads: call.threads.clone(),
            not_before: i64::MAX,
            client_token: rail_core::ops::client_token(),
        };
        let OutboxOutcome::Enqueued { rows, applied } =
            store.outbox_write(&OutboxWrite::Enqueue(new), 0).unwrap()
        else {
            panic!()
        };
        Ok(ActionApplyResponse { rows, applied })
    }));
}

async fn tick(f: &mut Fixture, n: usize) {
    for _ in 0..n {
        let effects = f.screen.update(Msg::Tick);
        settle(&mut f.screen, effects).await;
    }
}

#[tokio::test]
async fn e_archives_the_cursor_row_and_it_leaves_the_inbox() {
    let mut f = fixture(3).await;
    applying(&f);
    let top = f.screen.list.selected().unwrap().key.thread_id;
    press(&mut f, KeyCode::Char('e')).await;
    assert_eq!(
        applies(&f),
        [FakeCall::Apply {
            account: "work".into(),
            threads: vec![top],
            action: MailAction::Archive {},
            grace: true
        }]
    );
    assert!(
        !thread_ids(&f).contains(&top),
        "gone from the unified inbox"
    );
    assert!(
        f.screen.status_line().contains("archived 1 · z undoes"),
        "{}",
        f.screen.status_line()
    );
}

#[tokio::test]
async fn s_and_u_toggle_from_what_the_rows_show() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char('s')).await;
    press(&mut f, KeyCode::Char('s')).await;
    press(&mut f, KeyCode::Char('u')).await;
    press(&mut f, KeyCode::Char('u')).await;
    let actions: Vec<MailAction> = applies(&f)
        .into_iter()
        .map(|c| match c {
            FakeCall::Apply { action, .. } => action,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        actions,
        [
            MailAction::Star { on: true },
            MailAction::Star { on: false },
            MailAction::Read { on: true },
            MailAction::Read { on: false }
        ]
    );
}

#[tokio::test]
async fn a_selection_survives_a_requery_and_a_scope_change_and_an_action_clears_it() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char(' ')).await;
    assert_eq!(f.screen.actions.selection.len(), 2);
    assert!(
        f.screen.header_left().ends_with("· 2 selected"),
        "{}",
        f.screen.header_left()
    );
    store_changed(&mut f).await;
    press(&mut f, KeyCode::Char('1')).await;
    assert_eq!(f.screen.actions.selection.len(), 2, "kept across both");
    press(&mut f, KeyCode::Char('e')).await;
    // The two newest unified rows are one thread of each account: one call apiece.
    let sent: usize = applies(&f)
        .iter()
        .map(|c| match c {
            FakeCall::Apply { threads, .. } => threads.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(sent, 2);
    assert!(f.screen.actions.selection.is_empty());
}

#[tokio::test]
async fn esc_clears_the_selection_in_the_list() {
    let mut f = fixture(3).await;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Esc).await;
    assert!(f.screen.actions.selection.is_empty());
}

#[tokio::test]
async fn a_selection_across_accounts_sends_one_call_each_and_one_undo() {
    let mut f = fixture(2).await;
    applying(&f);
    // The unified inbox: wm1, wm0, hm1, hm0 newest first; select all four.
    for _ in 0..4 {
        press(&mut f, KeyCode::Char(' ')).await;
    }
    press(&mut f, KeyCode::Char('e')).await;
    let calls = applies(&f);
    let accounts: Vec<&str> = calls
        .iter()
        .map(|c| match c {
            FakeCall::Apply { account, .. } => account.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(accounts, ["work", "home"]);
    assert!(f.screen.status_line().contains("archived 4 · z undoes"));
    press(&mut f, KeyCode::Char('z')).await;
    let undo: Vec<FakeCall> = f
        .fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::Undo { .. }))
        .cloned()
        .collect();
    let [FakeCall::Undo { rows }] = &undo[..] else {
        panic!("{undo:?}")
    };
    assert_eq!(rows.len(), 2, "both accounts' rows in one undo");
    assert!(f.screen.status_line().contains("undone"));
}

#[tokio::test]
async fn trash_asks_first_and_names_one_subject() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char('d')).await;
    assert!(applies(&f).is_empty(), "nothing before the answer");
    assert!(matches!(f.screen.modal, Some(Modal::Confirm(_))));
    press(&mut f, KeyCode::Char('n')).await;
    assert!(applies(&f).is_empty());
    press(&mut f, KeyCode::Char('d')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert!(matches!(
        &applies(&f)[..],
        [FakeCall::Apply {
            action: MailAction::Trash {},
            ..
        }]
    ));
}

#[tokio::test]
async fn z_after_the_window_or_when_too_late_says_so() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
    press(&mut f, KeyCode::Char('e')).await;
    tick(&mut f, UNDO_TICKS).await;
    assert!(
        !f.screen.status_line().contains("z undoes"),
        "the notice expired"
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert!(
        f.screen.status_line().contains("Too late to undo"),
        "{}",
        f.screen.status_line()
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
    press(&mut f, KeyCode::Char('e')).await;
    *f.fake.undo_answer.lock().unwrap() = Some(Ok(rail_core::ops::ActionUndoResponse {
        undone: vec![],
        too_late: vec![2],
    }));
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("too late to undo"));
}

/// A suspended terminal skips ticks: past `UNDO_GRACE` of wall time `z` is refused although
/// the tick count is still inside the window, and nothing is sent.
#[tokio::test]
async fn z_after_the_wall_clock_window_is_too_late_whatever_the_ticks() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char('e')).await;
    f.screen.pass_undo_deadline();
    press(&mut f, KeyCode::Char('z')).await;
    assert!(
        f.screen.status_line().contains("Too late to undo"),
        "{}",
        f.screen.status_line()
    );
    let undos = f
        .fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::Undo { .. }))
        .count();
    assert_eq!(undos, 0);
}

#[tokio::test]
async fn reader_keys_act_on_the_open_thread_and_archive_closes_it() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Enter).await;
    let open = f.screen.reader().unwrap().thread_id();
    let read: Vec<FakeCall> = applies(&f);
    assert_eq!(
        read,
        [FakeCall::Apply {
            account: "work".into(),
            threads: vec![open],
            action: MailAction::Read { on: true },
            grace: false
        }],
        "mark-read on open"
    );
    assert!(
        !f.screen.status_line().contains("undoes"),
        "no undo notice for it"
    );
    press(&mut f, KeyCode::Char('s')).await;
    assert!(f.screen.reader().is_some(), "star keeps it open");
    press(&mut f, KeyCode::Char('e')).await;
    assert!(f.screen.reader().is_none(), "archive closes it");
    let last = applies(&f).pop().unwrap();
    assert_eq!(
        last,
        FakeCall::Apply {
            account: "work".into(),
            threads: vec![open],
            action: MailAction::Archive {},
            grace: true
        }
    );
}

#[tokio::test]
async fn opening_a_read_thread_sends_nothing() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(applies(&f).len(), 1, "the second open finds it read");
}

#[tokio::test]
async fn the_status_bar_shows_queued_and_failed_rows() {
    let mut f = fixture(3).await;
    let mut s = status("work", "w@example.com", SyncPhase::Idle);
    s.outbox = OutboxDepth {
        pending: 2,
        failed: 1,
        sending: 0,
        saving: 0,
    };
    let effects = f
        .screen
        .update(Msg::Task(TaskResult::Mail(MailEvent::Daemon(
            DaemonEvent::Progress(Box::new(s)),
        ))));
    settle(&mut f.screen, effects).await;
    assert!(
        f.screen.status_line().contains("work ✓ 2 queued 1 failed"),
        "{}",
        f.screen.status_line()
    );
}

#[tokio::test]
async fn a_failure_shows_on_the_row_and_in_errors_where_enter_retries() {
    let mut f = fixture(3).await;
    let top = f.screen.list.selected().unwrap().key.thread_id;
    let failure = ActionFailed {
        account: "work".into(),
        row: 7,
        threads: vec![top],
        action: MailAction::Archive {},
        error: OpError::not_found("the mail provider no longer has it")
            .with_detail("Not\u{1b}[31m Found"),
        draft: None,
        client_token: "tok".into(),
    };
    let effects = f
        .screen
        .update(Msg::Task(TaskResult::Mail(MailEvent::ActionFailed(
            Box::new(failure),
        ))));
    settle(&mut f.screen, effects).await;
    let text = f.screen.actions.failures.get(&top).unwrap().1.clone();
    assert!(
        text.starts_with("Could not archive: the mail provider no longer has it"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'), "escaped: {text}");
    let effects = f.screen.run_palette(Target::Errors);
    settle(&mut f.screen, effects).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(
        f.fake
            .calls
            .lock()
            .unwrap()
            .contains(&FakeCall::Retry { row: token(7) })
    );
    assert!(
        !f.screen.actions.failures.contains_key(&top),
        "cleared by the retry"
    );
}

/// A failure the daemon sends again on a reconnect is listed once in `:errors`, and the
/// retry still works; once retried, a new failure of the row is listed again.
#[tokio::test]
async fn a_failure_sent_again_on_a_reconnect_is_listed_once() {
    let mut f = fixture(3).await;
    let top = f.screen.list.selected().unwrap().key.thread_id;
    let failure = || {
        MailEvent::ActionFailed(Box::new(ActionFailed {
            account: "work".into(),
            row: 7,
            threads: vec![top],
            action: MailAction::Archive {},
            error: OpError::not_found("the mail provider no longer has it"),
            draft: None,
            client_token: "tok".into(),
        }))
    };
    let retries = |f: &Fixture| {
        f.screen
            .status
            .errors()
            .filter(|e| e.retry == Some(token(7)))
            .count()
    };
    feed(&mut f, failure()).await;
    feed(&mut f, failure()).await;
    assert_eq!(retries(&f), 1);
    let effects = f.screen.run_palette(Target::Errors);
    settle(&mut f.screen, effects).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(
        f.fake
            .calls
            .lock()
            .unwrap()
            .contains(&FakeCall::Retry { row: token(7) })
    );
    feed(&mut f, failure()).await;
    assert_eq!(
        retries(&f),
        1,
        "failed again after the retry: offered again"
    );
}

fn token(row: i64) -> rail_core::ops::RowToken {
    rail_core::ops::RowToken {
        row,
        client_token: "tok".into(),
    }
}

async fn feed(f: &mut Fixture, event: MailEvent) {
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
}

fn applied(seq: u64, account: &str, row: i64) -> MailEvent {
    MailEvent::Applied {
        call: Box::new(ApplyCall {
            seq,
            account: account.into(),
            threads: vec![1],
            action: MailAction::Archive {},
            grace: true,
            client_token: format!("tok-{row}"),
            snooze: None,
        }),
        result: Ok(ActionApplyResponse {
            rows: vec![row],
            applied: 1,
        }),
    }
}

fn undo_calls(f: &Fixture) -> Vec<Vec<i64>> {
    f.fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|c| match c {
            FakeCall::Undo { rows } => Some(rows.iter().map(|r| r.row).collect()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn z_names_each_row_with_the_token_its_call_was_sent_under() {
    let mut f = fixture(2).await;
    press(&mut f, KeyCode::Char(' ')).await;
    let _unrun = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    feed(&mut f, applied(1, "work", 10)).await;
    press(&mut f, KeyCode::Char('z')).await;
    let calls = f.fake.calls.lock().unwrap();
    assert!(
        calls.contains(&FakeCall::Undo {
            rows: vec![token_of(10, "tok-10")]
        }),
        "{calls:?}"
    );
}

fn token_of(row: i64, token: &str) -> rail_core::ops::RowToken {
    rail_core::ops::RowToken {
        row,
        client_token: token.into(),
    }
}

#[tokio::test]
async fn z_between_two_accounts_answers_undoes_both() {
    let mut f = fixture(2).await;
    for _ in 0..4 {
        press(&mut f, KeyCode::Char(' ')).await;
    }
    // The two calls go out but their answers are fed by hand, one after the key.
    let _unrun = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    feed(&mut f, applied(1, "work", 10)).await;
    press(&mut f, KeyCode::Char('z')).await;
    assert_eq!(undo_calls(&f), [vec![10]]);
    assert!(
        f.screen.status_line().contains("undoing…"),
        "{}",
        f.screen.status_line()
    );
    feed(&mut f, applied(1, "home", 11)).await;
    assert_eq!(undo_calls(&f), [vec![10], vec![11]]);
    assert!(
        f.screen.status_line().contains("undone"),
        "{}",
        f.screen.status_line()
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

#[tokio::test]
async fn z_before_any_account_answers_waits_and_undoes_each_answer() {
    let mut f = fixture(2).await;
    for _ in 0..4 {
        press(&mut f, KeyCode::Char(' ')).await;
    }
    let _unrun = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    press(&mut f, KeyCode::Char('z')).await;
    assert!(undo_calls(&f).is_empty());
    feed(&mut f, applied(1, "work", 10)).await;
    feed(&mut f, applied(1, "home", 11)).await;
    assert_eq!(undo_calls(&f), [vec![10], vec![11]]);
    assert!(f.screen.status_line().contains("undone"));
}

#[tokio::test]
async fn a_failed_request_keeps_the_selection_and_leaves_nothing_to_undo() {
    let mut f = fixture(3).await;
    *f.fake.on_apply.lock().unwrap() =
        Some(Box::new(|_| Err(OpError::not_found("the daemon said no"))));
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char('e')).await;
    assert_eq!(f.screen.actions.selection.len(), 2, "put back");
    assert!(f.screen.status_line().contains("Could not archive"));
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

#[tokio::test]
async fn a_failed_account_keeps_its_error_on_the_bar_while_the_other_answers() {
    let mut f = fixture(2).await;
    for _ in 0..4 {
        press(&mut f, KeyCode::Char(' ')).await;
    }
    let _unrun = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    let failed = MailEvent::Applied {
        call: Box::new(ApplyCall {
            seq: 1,
            account: "work".into(),
            threads: vec![1],
            action: MailAction::Archive {},
            grace: true,
            client_token: "tok-9".into(),
            snooze: None,
        }),
        result: Err(OpError::not_found("the daemon said no")),
    };
    feed(&mut f, failed).await;
    feed(&mut f, applied(1, "home", 11)).await;
    let line = f.screen.status_line();
    assert!(
        line.contains("Could not archive: the daemon said no"),
        "{line}"
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert_eq!(undo_calls(&f), [vec![11]], "home's row is still undone");
}

#[tokio::test]
async fn an_action_in_the_reader_keeps_a_selection_it_did_not_use() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Char(' ')).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(f.screen.reader().is_some());
    press(&mut f, KeyCode::Char('s')).await;
    assert_eq!(f.screen.actions.selection.len(), 2);
}

#[tokio::test]
async fn u_right_after_opening_marks_unread() {
    let mut f = fixture(3).await;
    applying(&f);
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('u')).await;
    let last = applies(&f).pop().unwrap();
    assert!(
        matches!(
            last,
            FakeCall::Apply {
                action: MailAction::Read { on: false },
                ..
            }
        ),
        "{last:?}"
    );
}

fn applied_rows(seq: u64, account: &str, rows: Vec<i64>) -> MailEvent {
    MailEvent::Applied {
        call: Box::new(ApplyCall {
            seq,
            account: account.into(),
            threads: vec![1],
            action: MailAction::Archive {},
            grace: true,
            client_token: "tok".into(),
            snooze: None,
        }),
        result: Ok(ActionApplyResponse { rows, applied: 1 }),
    }
}

fn apply_failed(seq: u64, account: &str) -> MailEvent {
    MailEvent::Applied {
        call: Box::new(ApplyCall {
            seq,
            account: account.into(),
            threads: vec![1],
            action: MailAction::Archive {},
            grace: true,
            client_token: "tok".into(),
            snooze: None,
        }),
        result: Err(OpError::not_found("gone")),
    }
}

/// Two accounts' calls sent (not run), `z` pressed, then the first account answers.
async fn undoing_with_one_call_left(f: &mut Fixture) {
    for _ in 0..4 {
        press(f, KeyCode::Char(' ')).await;
    }
    let _unrun = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    press(f, KeyCode::Char('z')).await;
    feed(f, applied(1, "work", 10)).await;
    assert_eq!(undo_calls(f), [vec![10]]);
    assert!(f.screen.status_line().contains("undoing…"));
}

#[tokio::test]
async fn the_last_call_failing_still_reports_the_undo_that_happened() {
    let mut f = fixture(2).await;
    *f.fake.undo_answer.lock().unwrap() = Some(Ok(rail_core::ops::ActionUndoResponse {
        undone: vec![],
        too_late: vec![10],
    }));
    undoing_with_one_call_left(&mut f).await;
    feed(&mut f, apply_failed(1, "home")).await;
    assert!(
        f.screen.status_line().contains("too late to undo"),
        "{}",
        f.screen.status_line()
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert!(f.screen.status_line().contains("Nothing to undo"));
}

#[tokio::test]
async fn the_last_call_returning_no_rows_still_reports_the_undo_that_happened() {
    let mut f = fixture(2).await;
    undoing_with_one_call_left(&mut f).await;
    feed(&mut f, applied_rows(1, "home", vec![])).await;
    assert!(
        f.screen.status_line().contains("undone"),
        "{}",
        f.screen.status_line()
    );
}

#[tokio::test]
async fn a_stale_undo_answer_does_not_touch_the_newer_action() {
    let mut f = fixture(3).await;
    let _first = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    feed(&mut f, applied(1, "work", 10)).await;
    let _undo = f.screen.update(Msg::Key(key(KeyCode::Char('z'))));
    let _second = f.screen.update(Msg::Key(key(KeyCode::Char('e'))));
    feed(&mut f, applied(2, "work", 20)).await;
    let stale = MailEvent::Undone {
        seq: 1,
        result: Ok(rail_core::ops::ActionUndoResponse {
            undone: vec![10],
            too_late: vec![],
        }),
    };
    feed(&mut f, stale).await;
    assert!(
        f.screen.status_line().contains("archived 1 · z undoes"),
        "{}",
        f.screen.status_line()
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert_eq!(undo_calls(&f), [vec![20]], "the newer rows stay undoable");
    assert!(f.screen.status_line().contains("undone"));
}

#[tokio::test]
async fn an_undo_answer_with_no_action_says_nothing() {
    let mut f = fixture(3).await;
    let stray = MailEvent::Undone {
        seq: 9,
        result: Ok(rail_core::ops::ActionUndoResponse {
            undone: vec![],
            too_late: vec![],
        }),
    };
    feed(&mut f, stray).await;
    assert!(!f.screen.status_line().contains("undone"));
}

#[tokio::test]
async fn the_trash_prompt_at_90_and_150_columns() {
    for width in [90, 150] {
        let mut f = fixture(3).await;
        press(&mut f, KeyCode::Char('d')).await;
        let shot = crate::tui::test_util::snapshot(width, 24, |frame| {
            f.screen.view(frame, &crate::tui::theme::Theme::default())
        });
        insta::assert_snapshot!(format!("trash_prompt_{width}"), shot);
    }
}

/// `rules`, each a name, a pattern and a template, compiled as the mail screen holds them.
fn link_actions(rules: &[(&str, &str, &str)]) -> Vec<rail_core::config::CompiledLinkAction> {
    let config = rail_core::config::Config {
        link_actions: rules
            .iter()
            .map(|(name, pattern, url)| rail_core::config::LinkAction {
                name: (*name).to_string(),
                pattern: (*pattern).to_string(),
                url: (*url).to_string(),
            })
            .collect(),
        ..rail_core::config::Config::default()
    };
    rail_core::config::compile_link_actions(&config).unwrap()
}

/// With nothing configured the key says so rather than doing nothing, so a reader who set a
/// rule in the wrong file learns where to look.
#[tokio::test]
async fn open_link_without_rules_says_so() {
    let mut f = fixture(3).await;
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    assert!(f.screen.modal.is_none());
    assert!(
        f.screen.status_line().contains("No link actions"),
        "{}",
        f.screen.status_line()
    );
}

/// The subjects the fixture seeds are `subject wm<n>`, so a rule over that number stands in
/// for a tracker id: one match opens through the same confirmation as any link.
#[tokio::test]
async fn open_link_confirms_a_single_match() {
    let mut f = fixture(3).await;
    f.screen.ctx.link_actions =
        link_actions(&[("tracker", r"m(\d+)", "https://tracker.example/t/{1}")]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    assert!(matches!(f.screen.modal, Some(Modal::Confirm(_))));
}

/// Two rules matching the same subject offer both addresses, by number: the question is
/// which record was meant, and only the reader knows.
#[tokio::test]
async fn open_link_offers_every_match_by_number() {
    let mut f = fixture(3).await;
    f.screen.ctx.link_actions = link_actions(&[
        ("tracker", r"m(\d+)", "https://tracker.example/t/{1}"),
        ("wiki", r"subject (\w+)", "https://other.example/s/{1}"),
    ]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    let Some(Modal::LinkNumber(prompt)) = &f.screen.modal else {
        panic!("two matches ask by number");
    };
    // Nothing else on screen says what 1 and 2 stand for, so the prompt has to: the rule
    // that found each address, and the address itself.
    let rows = prompt.rows();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].contains("tracker") && rows[0].contains("https://tracker.example/t/"));
    assert!(rows[1].contains("wiki") && rows[1].contains("https://other.example/s/"));
}

/// How that listing looks, at the width the prompt is drawn to.
#[tokio::test]
async fn open_link_prompt_lists_every_match() {
    let mut f = fixture(3).await;
    f.screen.ctx.link_actions = link_actions(&[
        ("tracker", r"m(\d+)", "https://tracker.example/t/{1}"),
        ("wiki", r"subject (\w+)", "https://other.example/s/{1}"),
    ]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    let shot = crate::tui::test_util::snapshot(100, 24, |frame| {
        f.screen.view(frame, &crate::tui::theme::Theme::default())
    });
    insta::assert_snapshot!("open_link_prompt", shot);
}

/// The number has to reach the address it was listed against, all the way to the browser:
/// choosing 2 confirms the second rule's address and opens that one.
#[tokio::test]
async fn open_link_number_two_opens_the_second_address() {
    let mut f = fixture(3).await;
    let (opener, opened) = crate::tui::test_util::recording_opener();
    f.screen.ctx.opener = opener;
    f.screen.ctx.link_actions = link_actions(&[
        ("tracker", r"m(\d+)", "https://tracker.example/t/{1}"),
        ("wiki", r"subject (\w+)", "https://other.example/s/{1}"),
    ]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    press(&mut f, KeyCode::Char('2')).await;
    press(&mut f, KeyCode::Enter).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "a chosen number is confirmed like any other link"
    );
    press(&mut f, KeyCode::Char('y')).await;
    let opened = opened.lock().unwrap();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(
        opened[0].as_str().starts_with("https://other.example/s/"),
        "{}",
        opened[0]
    );
}

/// Two rules that read a subject the same way are one choice, not two: the question the
/// picker asks is where to go, and the answer is the same either way.
#[tokio::test]
async fn open_link_offers_one_address_once() {
    let mut f = fixture(3).await;
    f.screen.ctx.link_actions = link_actions(&[
        ("tracker", r"m(\d+)", "https://tracker.example/t/{1}"),
        ("again", r"subject wm(\d+)", "https://tracker.example/t/{1}"),
    ]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "one address means one confirmation, not a prompt"
    );
}

/// A subject no rule matches is a notice, not a modal: nothing to open is not an error.
#[tokio::test]
async fn open_link_says_when_nothing_matches() {
    let mut f = fixture(3).await;
    f.screen.ctx.link_actions =
        link_actions(&[("tracker", r"\(#(\d+)\)", "https://tracker.example/t/{1}")]);
    press(&mut f, KeyCode::Char('g')).await;
    press(&mut f, KeyCode::Char('t')).await;
    assert!(f.screen.modal.is_none());
    assert!(
        f.screen.status_line().contains("matches a link action"),
        "{}",
        f.screen.status_line()
    );
}
