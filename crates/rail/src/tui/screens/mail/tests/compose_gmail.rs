//! `s` on the review: the draft saved to Gmail's Drafts, its undo, a refused push, a push that
//! fails later into `:errors`, and the key named on the bottom row.

use rail_core::error::ErrorClass;
use rail_core::ops::{ActionFailed, DraftPushResponse, DraftSaveResponse, ErrorCode, RowToken};

use super::compose::{calls_of, compose_fixture, compose_open, composing, editor_appends, saves};
use super::*;
use crate::tui::modals::Target;

fn pushes(f: &Fixture) -> Vec<(i64, String)> {
    calls_of(f, |c| matches!(c, FakeCall::DraftPush { .. }))
        .into_iter()
        .filter_map(|c| match c {
            FakeCall::DraftPush {
                draft,
                client_token,
            } => Some((draft, client_token)),
            _ => None,
        })
        .collect()
}

fn undos(f: &Fixture) -> Vec<Vec<RowToken>> {
    calls_of(f, |c| matches!(c, FakeCall::Undo { .. }))
        .into_iter()
        .filter_map(|c| match c {
            FakeCall::Undo { rows } => Some(rows),
            _ => None,
        })
        .collect()
}

/// The review of a reply to the newest thread, saved as draft 1.
async fn review(f: &mut Fixture) {
    editor_appends(f, "x\n");
    press(f, KeyCode::Enter).await;
    press(f, KeyCode::Char('r')).await;
    assert!(compose_open(f));
}

#[tokio::test]
async fn s_saves_pending_text_then_pushes_and_closes_with_an_undo_notice() {
    let mut f = compose_fixture().await;
    let mut calls = 0;
    *f.fake.on_save.lock().unwrap() = Some(Box::new(move |_| {
        calls += 1;
        if calls == 2 {
            Err(OpError::new(
                ErrorCode::Daemon,
                ErrorClass::Transient,
                "the connection to the daemon closed",
            ))
        } else {
            Ok(DraftSaveResponse {
                draft: 1,
                message_id: "<x@example.com>".into(),
                attachments: vec![],
            })
        }
    }));
    review(&mut f).await; // save 1 ok, the editor, save 2 fails: the text is unsaved
    assert!(composing(&f).body.contains('x'));
    assert!(pushes(&f).is_empty());
    press(&mut f, KeyCode::Char('s')).await;
    assert_eq!(saves(&f).len(), 3, "the unsaved text is saved first");
    let order: Vec<bool> = calls_of(&f, |c| {
        matches!(c, FakeCall::DraftSave { .. } | FakeCall::DraftPush { .. })
    })
    .iter()
    .map(|c| matches!(c, FakeCall::DraftPush { .. }))
    .collect();
    assert_eq!(
        order,
        [false, false, false, true],
        "the push after the save"
    );
    let pushed = pushes(&f);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].0, 1);
    assert!(!pushed[0].1.is_empty(), "a fresh token");
    assert!(!compose_open(&f), "the review closes");
    let line = f.screen.status_line();
    assert!(line.contains("saved to Gmail · z undoes"), "{line}");
}

#[tokio::test]
async fn s_on_a_saved_draft_pushes_without_saving_again() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    let before = saves(&f).len();
    press(&mut f, KeyCode::Char('s')).await;
    assert_eq!(saves(&f).len(), before, "nothing to save");
    assert_eq!(pushes(&f).len(), 1);
    assert_eq!(pushes(&f)[0].0, 1);
    assert!(!compose_open(&f));
}

#[tokio::test]
async fn z_in_the_grace_cancels_the_push_and_the_draft_stays_local() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    press(&mut f, KeyCode::Char('s')).await;
    let (_, token) = pushes(&f).remove(0);
    press(&mut f, KeyCode::Char('z')).await;
    let undone = undos(&f);
    assert_eq!(undone.len(), 1);
    assert_eq!(undone[0].len(), 1);
    assert_eq!(undone[0][0].client_token, token, "the push's own token");
    let line = f.screen.status_line();
    assert!(line.contains("kept on this computer"), "{line}");
    assert!(!compose_open(&f), "the review does not reopen");
    assert!(calls_of(&f, |c| matches!(c, FakeCall::DraftDiscard { .. })).is_empty());
}

#[tokio::test]
async fn a_second_s_in_the_grace_owns_the_undo_with_its_own_token() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    press(&mut f, KeyCode::Char('s')).await;
    press(&mut f, KeyCode::Char('r')).await;
    assert!(compose_open(&f));
    // The daemon answers the pending row again, as for a second push of the same draft.
    let row = 1;
    *f.fake.push_answer.lock().unwrap() = Some(Ok(DraftPushResponse { row }));
    press(&mut f, KeyCode::Char('s')).await;
    let pushed = pushes(&f);
    assert_eq!(pushed.len(), 2);
    assert_ne!(pushed[0].1, pushed[1].1, "each push has its own token");
    press(&mut f, KeyCode::Char('z')).await;
    let undone = undos(&f);
    assert_eq!(
        undone,
        [vec![RowToken {
            row,
            client_token: pushed[1].1.clone()
        }]],
        "z names the row with the newest token"
    );
    assert!(f.screen.status_line().contains("kept on this computer"));
}

#[tokio::test]
async fn a_refused_push_keeps_the_review_open_with_the_error() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    *f.fake.push_answer.lock().unwrap() = Some(Err(OpError::invalid_input(
        "draft: there is nothing to save",
    )));
    press(&mut f, KeyCode::Char('s')).await;
    assert!(compose_open(&f), "the review stays");
    assert_eq!(composing(&f).errors, ["draft: there is nothing to save"]);
    let line = f.screen.status_line();
    assert!(
        line.contains("Could not save to Gmail: draft: there is nothing to save"),
        "{line}"
    );
    press(&mut f, KeyCode::Char('z')).await;
    assert!(undos(&f).is_empty(), "nothing to undo");
}

#[tokio::test]
async fn a_push_that_fails_later_lands_in_errors_with_retry() {
    let mut f = compose_fixture().await;
    let failure = ActionFailed {
        account: "work".into(),
        row: 9,
        threads: vec![],
        action: rail_core::ops::Action::PushDraft { draft: 1 },
        error: OpError::new(
            ErrorCode::Provider,
            ErrorClass::Permanent,
            "the mail provider refused the draft",
        ),
        draft: Some(1),
        client_token: "tok-push".into(),
    };
    let event = MailEvent::ActionFailed(Box::new(failure));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    let line = f.screen.status_line();
    assert!(
        line.contains("Could not save the draft to Gmail: the mail provider refused the draft"),
        "{line}"
    );
    assert!(!compose_open(&f), "unlike a send, nothing reopens");
    assert_eq!(f.screen.error_count(), 1);
    let effects = f.screen.run_palette(Target::Errors);
    settle(&mut f.screen, effects).await;
    press(&mut f, KeyCode::Enter).await;
    let retried = calls_of(&f, |c| matches!(c, FakeCall::Retry { .. }));
    assert_eq!(
        retried,
        [FakeCall::Retry {
            row: RowToken {
                row: 9,
                client_token: "tok-push".into()
            }
        }]
    );
}

#[tokio::test]
async fn the_review_names_the_push_key() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    let bottom = |f: &Fixture| {
        let rows = crate::tui::test_util::render_lines(120, 30, |frame| {
            f.screen.view(frame, &Theme::default());
        });
        rows[29].clone()
    };
    let hint = bottom(&f);
    assert!(hint.contains("y send  s save to Gmail  e edit"), "{hint}");
    press(&mut f, KeyCode::Char('q')).await;
    let text = "[keys]\n\"compose.push\" = \"S\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    f.screen.ctx.keymap =
        Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap());
    press(&mut f, KeyCode::Char('r')).await;
    expire_notice(&mut f).await;
    let hint = bottom(&f);
    assert!(hint.contains("y send  S save to Gmail  e edit"), "{hint}");
}

#[tokio::test]
async fn the_review_draws_inside_the_frame_with_its_keys_in_the_hint_bar() {
    let mut f = compose_fixture().await;
    review(&mut f).await;
    let shot = crate::tui::test_util::snapshot(150, 40, |frame| {
        f.screen.view(frame, &Theme::default());
    });
    insta::assert_snapshot!("compose_frame_150", shot);
}
