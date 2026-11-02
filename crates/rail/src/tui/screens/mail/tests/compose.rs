//! c, r, R, f: prompts, the editor round trip, the review, send and undo, keep, discard,
//! attach and remove, resume from :drafts, the failure paths.

use rail_core::error::ErrorClass;
use rail_core::ops::drafts::{AttachmentSummary, DraftAttachment};
use rail_core::ops::{ActionFailed, DraftSaveResponse, ErrorCode};
use rail_core::provider::Body;
use rail_core::store::drafts::{DraftKind, DraftWrite, Recipient};

use super::*;
use crate::tui::modals::Target;
use crate::tui::screens::compose::RowKind;

pub(super) async fn type_text(f: &mut Fixture, text: &str) {
    for c in text.chars() {
        press(f, KeyCode::Char(c)).await;
    }
}

pub(super) fn saves(f: &Fixture) -> Vec<FakeCall> {
    f.fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::DraftSave { .. }))
        .cloned()
        .collect()
}

pub(super) fn calls_of(f: &Fixture, keep: fn(&FakeCall) -> bool) -> Vec<FakeCall> {
    let calls = f.fake.calls.lock().unwrap();
    calls.iter().filter(|c| keep(c)).cloned().collect()
}

pub(super) fn compose_open(f: &Fixture) -> bool {
    f.screen.compose.is_some()
}

pub(super) fn composing(f: &Fixture) -> &crate::tui::screens::compose::model::Composing {
    f.screen.compose.as_ref().unwrap().composing()
}

/// The fake editor appends `text` to the file and exits 0.
pub(super) fn editor_appends(f: &Fixture, text: &'static str) {
    f.editor.set(move |path| {
        let mut current = std::fs::read_to_string(path).unwrap();
        current.push_str(text);
        std::fs::write(path, current).unwrap();
        Ok(0)
    });
}

/// The fake editor types `text` at the top of the body (above a signature) and exits 0.
fn editor_types_above(f: &Fixture, text: &'static str) {
    f.editor.set(move |path| {
        let current = std::fs::read_to_string(path).unwrap();
        std::fs::write(path, current.replacen("\n\n", &format!("\n\n{text}"), 1)).unwrap();
        Ok(0)
    });
}

impl Fixture {
    /// A draft row of `work` written straight into the store; it must get `id`.
    fn seed_draft(&self, id: i64, subject: &str, body: &str) {
        let write = DraftWrite {
            id: None,
            account_id: 1,
            kind: DraftKind::Reply,
            thread_id: None,
            remote_thread_id: None,
            in_reply_to: None,
            references: None,
            to: vec![Recipient {
                name: Some("Sender".into()),
                address: "sender@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body: body.into(),
            attachments: vec![],
            message_id: "<seeded@example.com>".into(),
            source_remote_id: None,
        };
        let made = writer(self).draft_upsert(&write, BASE_MS / 1000).unwrap();
        assert_eq!(made, Some(id), "drafts are numbered in order");
    }
}

/// `fixture(3)` with the newest thread's body stored and `sam@example.com` in the address
/// book.
pub(super) async fn compose_fixture() -> Fixture {
    let f = fixture(3).await;
    let mut store = writer(&f);
    let body = Body {
        text: "Hi there\nsecond line".into(),
        ..Body::default()
    };
    store
        .apply(
            &Batch {
                bodies: vec![("wm2".into(), body)],
                ..Batch::new(1)
            },
            BASE_MS / 1000,
        )
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO addresses (address, name, hits, last_seen) VALUES ('sam@example.com', NULL, 5, 1)",
            [],
        )
        .unwrap();
    f
}

#[tokio::test]
async fn c_prompts_to_and_subject_then_edits_saves_and_shows_the_review() {
    let mut f = compose_fixture().await;
    editor_types_above(&f, "Hello Sam\n");
    press(&mut f, KeyCode::Char('1')).await; // work's inbox: no account question
    press(&mut f, KeyCode::Char('c')).await;
    assert_eq!(
        f.screen.compose.as_ref().and_then(|c| c.prompt_label()),
        Some("To")
    );
    type_text(&mut f, "sa").await;
    assert!(f.screen.compose.as_ref().unwrap().captures_text());
    // The store's suggestion sam@example.com shows; Tab highlights it and Enter takes it.
    press(&mut f, KeyCode::Tab).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(
        f.screen.compose.as_ref().and_then(|c| c.prompt_label()),
        Some("Subject")
    );
    type_text(&mut f, "Lunch").await;
    press(&mut f, KeyCode::Enter).await;
    // Save, edit, save again, review.
    let saved = saves(&f);
    assert_eq!(saved.len(), 2, "once before the editor, once after");
    assert!(
        matches!(&saved[0], FakeCall::DraftSave { kind: DraftKind::New, to, subject, .. } if to == &["sam@example.com".to_string()] && subject == "Lunch"),
        "{saved:?}"
    );
    assert!(compose_open(&f));
    assert!(
        !f.screen.compose.as_ref().unwrap().captures_text(),
        "the review, not a prompt"
    );
    assert!(composing(&f).body.contains("Hello Sam"));
    assert!(
        composing(&f).body.ends_with("-- \nwork sig\n"),
        "the signature was prefilled: {:?}",
        composing(&f).body
    );
    assert!(
        !f.paths.compose_dir().join("1.eml").exists(),
        "the compose file is removed after parsing"
    );
}

#[tokio::test]
async fn in_the_unified_inbox_c_asks_which_account_first() {
    let mut f = compose_fixture().await;
    press(&mut f, KeyCode::Char('c')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Picker(_))),
        "an account picker"
    );
    press(&mut f, KeyCode::Down).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(
        f.screen.compose.as_ref().and_then(|c| c.prompt_label()),
        Some("To")
    );
    assert_eq!(composing(&f).account_address, "h@example.com");
}

#[tokio::test]
async fn esc_in_the_opening_prompts_drops_the_message_without_a_call() {
    let mut f = compose_fixture().await;
    press(&mut f, KeyCode::Char('1')).await;
    press(&mut f, KeyCode::Char('c')).await;
    press(&mut f, KeyCode::Esc).await;
    assert!(!compose_open(&f));
    assert!(saves(&f).is_empty());
    assert!(calls_of(&f, |c| matches!(c, FakeCall::DraftDiscard { .. })).is_empty());
    assert_eq!(f.screen.focus, Focus::List);
}

#[tokio::test]
async fn r_prefills_from_the_open_thread_and_the_editor_opens_at_once() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await; // open the newest thread (its body is seeded)
    press(&mut f, KeyCode::Char('r')).await;
    let saved = saves(&f);
    assert!(matches!(
        &saved[0],
        FakeCall::DraftSave {
            kind: DraftKind::Reply,
            ..
        }
    ));
    let composing = composing(&f);
    assert_eq!(composing.kind, DraftKind::Reply);
    assert!(composing.headers.subject.starts_with("Re: "));
    assert!(composing.body.contains("wrote:\n> "), "{}", composing.body);
    assert!(compose_open(&f));
    assert!(f.screen.reader().is_some(), "the reader stays underneath");
}

#[tokio::test]
async fn r_from_the_list_fetches_a_missing_body_first() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Down).await; // a thread whose body is not seeded
    press(&mut f, KeyCode::Char('r')).await;
    assert!(
        f.fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, FakeCall::Fetch { .. })),
        "body.fetch first"
    );
    assert!(compose_open(&f), "then the editor and the review");
}

#[tokio::test]
async fn y_sends_closes_and_z_reopens_the_review_after_an_undo() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "x\n");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert!(!compose_open(&f));
    let sends = calls_of(&f, |c| matches!(c, FakeCall::DraftSend { .. }));
    assert_eq!(sends.len(), 1);
    assert!(
        f.screen.status_line().contains("sent · z undoes"),
        "{}",
        f.screen.status_line()
    );
    press(&mut f, KeyCode::Char('z')).await;
    let undos = calls_of(&f, |c| matches!(c, FakeCall::Undo { .. }));
    let FakeCall::DraftSend { client_token, .. } = &sends[0] else {
        unreachable!()
    };
    assert!(matches!(&undos[0], FakeCall::Undo { rows } if &rows[0].client_token == client_token));
    assert!(compose_open(&f), "the review is back");
    assert!(f.screen.status_line().contains("undone"));
}

#[tokio::test]
async fn q_keeps_the_draft_and_resumes_it_from_drafts() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "later\n");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('q')).await;
    assert!(!compose_open(&f));
    assert!(f.screen.status_line().contains("draft kept"));
    f.seed_draft(1, "Re: subject w3", "later\n");
    let effects = f.screen.run_palette(Target::Drafts);
    settle_capturing(&mut f.screen, effects, &f.last_theme, &f.editor).await;
    assert!(matches!(f.screen.modal, Some(Modal::Picker(_))));
    press(&mut f, KeyCode::Enter).await;
    assert!(compose_open(&f));
    assert_eq!(composing(&f).draft, Some(1));
    assert!(composing(&f).body.contains("later"));
}

#[tokio::test]
async fn d_asks_then_discards_and_a_non_zero_editor_exit_asks_to_keep_the_text() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('d')).await;
    assert!(compose_open(&f), "asks first");
    let rows = crate::tui::test_util::render_lines(120, 30, |frame| {
        f.screen.view(frame, &Theme::default());
    });
    assert!(
        rows[29].starts_with("Discard this draft? (y/n)"),
        "{}",
        rows[29]
    );
    press(&mut f, KeyCode::Char('y')).await;
    assert!(!compose_open(&f));
    assert!(
        f.fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, FakeCall::DraftDiscard { draft: 1 }))
    );
    assert!(f.screen.status_line().contains("draft discarded"));
    // A non-zero exit.
    f.editor.set(|path| {
        std::fs::write(path, "To: a@b.c\nCc: \nBcc: \nSubject: s\n\ntyped\n").unwrap();
        Ok(130)
    });
    press(&mut f, KeyCode::Char('r')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "Keep what you wrote?"
    );
    press(&mut f, KeyCode::Char('y')).await;
    assert!(composing(&f).body.contains("typed"));
}

#[tokio::test]
async fn a_attaches_through_the_daemon_and_x_removes() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"n").unwrap();
    *f.fake.on_save.lock().unwrap() = Some(Box::new(move |call| {
        Ok(DraftSaveResponse {
            draft: 1,
            message_id: "<x@example.com>".into(),
            attachments: call
                .request
                .attachments
                .iter()
                .filter_map(|a| match a {
                    DraftAttachment::File { path } => Some(AttachmentSummary {
                        name: "notes.txt".into(),
                        mime: "text/plain".into(),
                        size: 1,
                        path: Some(path.clone()),
                        message: None,
                        part: None,
                    }),
                    DraftAttachment::Part { .. } => None,
                })
                .collect(),
        })
    }));
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('a')).await;
    type_text(&mut f, &file.to_string_lossy()).await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(composing(&f).attachments.len(), 1);
    assert_eq!(composing(&f).attachments[0].name, "notes.txt");
    // Move onto the attachment row and remove it.
    while f.screen.compose.as_ref().unwrap().cursor_row_kind() != RowKind::Attachment(0) {
        press(&mut f, KeyCode::Char('j')).await;
    }
    press(&mut f, KeyCode::Char('x')).await;
    assert!(composing(&f).attachments.is_empty());
    assert_eq!(
        saves(&f).len(),
        4,
        "prefill save, after-editor save, attach save, remove save"
    );
}

#[tokio::test]
async fn a_failed_send_reopens_the_review_with_the_error() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert!(!compose_open(&f));
    f.seed_draft(1, "Re: subject w3", "");
    let failure = ActionFailed {
        account: "work".into(),
        row: 1,
        threads: vec![],
        action: rail_core::ops::Action::Send { draft: 1 },
        error: OpError::new(
            ErrorCode::Provider,
            ErrorClass::Permanent,
            "could not read an attachment of this message",
        ),
        draft: Some(1),
        client_token: "tok".into(),
    };
    let event = MailEvent::ActionFailed(Box::new(failure));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle_capturing(&mut f.screen, effects, &f.last_theme, &f.editor).await;
    assert!(compose_open(&f), "reopened");
    assert!(composing(&f).errors[0].contains("could not read an attachment"));
    assert!(f.screen.status_line().contains("Could not send"));
    assert_eq!(f.screen.error_count(), 1);
    // The answered message is not known for this draft: it still edits and saves.
    assert_eq!(composing(&f).reply_to, None);
    let before = saves(&f).len();
    editor_appends(&f, "fixed\n");
    press(&mut f, KeyCode::Char('e')).await;
    assert_eq!(saves(&f).len(), before + 1);
    assert!(
        composing(&f).errors.is_empty(),
        "the save cleared the error"
    );
    assert!(composing(&f).body.contains("fixed"));
    assert!(!f.paths.compose_dir().join("1.eml").exists());
}

#[tokio::test]
async fn a_refused_save_shows_the_error_on_the_review_and_keeps_editing() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    let mut calls = 0;
    *f.fake.on_save.lock().unwrap() = Some(Box::new(move |_| {
        calls += 1;
        if calls == 2 {
            Err(OpError::invalid_input("to: an address is malformed"))
        } else {
            Ok(DraftSaveResponse {
                draft: 1,
                message_id: "<x@example.com>".into(),
                attachments: vec![],
            })
        }
    }));
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    assert!(compose_open(&f));
    assert_eq!(composing(&f).errors, ["to: an address is malformed"]);
}

#[tokio::test]
async fn sending_shows_in_the_bar_while_a_send_is_in_flight() {
    let mut f = compose_fixture().await;
    let mut status = status("work", "w@example.com", SyncPhase::Idle);
    status.outbox.sending = 1;
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status)));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    assert!(
        f.screen.status_line().contains("sending…"),
        "{}",
        f.screen.status_line()
    );
}

#[tokio::test]
async fn saving_to_gmail_shows_in_the_bar_and_sending_wins_over_it() {
    let mut f = compose_fixture().await;
    let mut status = status("work", "w@example.com", SyncPhase::Idle);
    status.outbox.saving = 1;
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status.clone())));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    let line = f.screen.status_line();
    assert!(line.contains("saving to Gmail…"), "{line}");
    assert!(!line.contains("sending…"), "{line}");
    status.outbox.sending = 1;
    let event = MailEvent::Daemon(DaemonEvent::Progress(Box::new(status)));
    let effects = f.screen.update(Msg::Task(TaskResult::Mail(event)));
    settle(&mut f.screen, effects).await;
    let line = f.screen.status_line();
    assert!(line.contains("sending…"), "{line}");
    assert!(!line.contains("saving to Gmail…"), "{line}");
}

#[tokio::test]
async fn an_editor_that_cannot_run_keeps_the_draft_and_says_so() {
    let mut f = compose_fixture().await;
    f.editor
        .set(|_| Err("cannot run \"nope\": not found".into()));
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    assert!(
        f.screen.status_line().contains("cannot run"),
        "{}",
        f.screen.status_line()
    );
    assert!(
        compose_open(&f),
        "the review opens with the prefilled text so nothing is lost"
    );
}

#[tokio::test]
async fn a_compose_file_that_does_not_parse_reopens_the_editor_on_it() {
    let mut f = compose_fixture().await;
    let mut runs = 0;
    f.editor.set(move |path| {
        runs += 1;
        let text = std::fs::read_to_string(path).unwrap();
        let text = if runs == 1 {
            format!("X-Bogus: 1\n{text}")
        } else {
            text.replacen("X-Bogus: 1\n", "", 1)
                .replacen("\n\n", "\n\nfixed\n", 1)
        };
        std::fs::write(path, text).unwrap();
        Ok(0)
    });
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    assert!(
        f.screen.status.errors().any(|e| e.text.contains("line 1")),
        "the error names the line"
    );
    assert!(
        composing(&f).body.starts_with("fixed\n"),
        "{:?}",
        composing(&f).body
    );
    assert_eq!(saves(&f).len(), 2, "the bad text was never saved");
    assert!(!f.paths.compose_dir().join("1.eml").exists());
}

#[tokio::test]
async fn reply_all_and_forward_prefill_their_kind_and_subject() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('R')).await;
    assert_eq!(composing(&f).kind, DraftKind::ReplyAll);
    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Char('f')).await;
    let forward = composing(&f);
    assert_eq!(forward.kind, DraftKind::Forward);
    assert!(forward.headers.subject.starts_with("Fwd: "));
    assert!(forward.headers.to.is_empty());
    assert!(
        forward.body.contains("Forwarded message"),
        "{}",
        forward.body
    );
    let kinds: Vec<DraftKind> = saves(&f)
        .iter()
        .filter_map(|c| match c {
            FakeCall::DraftSave { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            DraftKind::ReplyAll,
            DraftKind::ReplyAll,
            DraftKind::Forward,
            DraftKind::Forward
        ]
    );
}

#[tokio::test]
async fn t_on_the_review_changes_the_headers_and_saves() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('t')).await;
    assert_eq!(
        f.screen.compose.as_ref().and_then(|c| c.prompt_label()),
        Some("To")
    );
    press_n(&mut f, KeyCode::Enter, 3).await; // To, Cc, Bcc as they are
    for _ in 0..40 {
        press(&mut f, KeyCode::Backspace).await;
    }
    type_text(&mut f, "New subject").await;
    press(&mut f, KeyCode::Enter).await;
    assert_eq!(composing(&f).headers.subject, "New subject");
    let last = saves(&f).pop();
    assert!(
        matches!(&last, Some(FakeCall::DraftSave { subject, .. }) if subject == "New subject"),
        "{last:?}"
    );
}

#[tokio::test]
async fn y_without_a_recipient_says_so_and_sends_nothing() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('f')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert!(compose_open(&f));
    assert!(calls_of(&f, |c| matches!(c, FakeCall::DraftSend { .. })).is_empty());
    assert!(
        f.screen.status_line().contains("Add a recipient"),
        "{}",
        f.screen.status_line()
    );
}

#[tokio::test]
async fn the_palette_writes_a_message_and_runs_the_composers_keys() {
    let mut f = compose_fixture().await;
    let entries = f.screen.palette_entries();
    let writes: Vec<_> = entries
        .iter()
        .filter(|e| e.text == "Write a new message")
        .collect();
    assert_eq!(writes.len(), 1, "one entry for c");
    assert_eq!(
        (writes[0].key.as_str(), &writes[0].target),
        ("c", &Target::Compose)
    );
    assert!(entries.iter().any(|e| e.target == Target::Drafts));
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    let in_review = f.screen.palette_entries();
    assert!(
        in_review
            .iter()
            .any(|e| e.target == Target::Action(Action::ComposeSend))
    );
    let effects = f.screen.run_palette(Target::Action(Action::ComposeClose));
    settle_capturing(&mut f.screen, effects, &f.last_theme, &f.editor).await;
    assert!(!compose_open(&f));
    assert_eq!(f.screen.focus, Focus::Reader);
}

#[tokio::test]
async fn closing_after_a_failed_save_asks_before_the_changes_are_lost() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "typed\n");
    let mut calls = 0;
    *f.fake.on_save.lock().unwrap() = Some(Box::new(move |_| {
        calls += 1;
        if calls % 2 == 0 {
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
    let file = f.paths.compose_dir().join("1.eml");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await; // save 1 ok, the editor, save 2 fails
    assert!(
        file.exists(),
        "the file keeps the text the failed save did not store"
    );
    press(&mut f, KeyCode::Char('q')).await;
    assert!(
        matches!(f.screen.modal, Some(Modal::Confirm(_))),
        "Discard the unsaved changes?"
    );
    press(&mut f, KeyCode::Char('n')).await;
    assert!(compose_open(&f), "no keeps the review");
    assert!(composing(&f).body.contains("typed"));
    press(&mut f, KeyCode::Char('e')).await; // save 3 ok
    assert!(!file.exists(), "a successful save removes the file");
    press(&mut f, KeyCode::Char('e')).await; // save 4 fails
    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Char('y')).await;
    assert!(!compose_open(&f));
    assert!(!file.exists());
    assert!(f.screen.status_line().contains("unsaved changes discarded"));
    // A composer opened afterwards starts saved: q closes at once.
    *f.fake.on_save.lock().unwrap() = None;
    press(&mut f, KeyCode::Char('r')).await;
    press(&mut f, KeyCode::Char('q')).await;
    assert!(!compose_open(&f));
    assert!(f.screen.status_line().contains("draft kept"));
}

#[tokio::test]
async fn the_review_names_the_users_keys() {
    let mut f = compose_fixture().await;
    let text = "[keys]\n\"compose.send\" = \"S\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    f.screen.ctx.keymap =
        Arc::new(Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap());
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    let rows = crate::tui::test_util::render_lines(120, 30, |frame| {
        f.screen.view(frame, &Theme::default());
    });
    let hint = &rows[29];
    assert!(
        hint.starts_with("S send  s save to Gmail  e edit  t headers"),
        "{hint}"
    );
}

#[tokio::test]
async fn an_open_prompt_shows_its_own_keys_in_the_bar() {
    let mut f = compose_fixture().await;
    editor_appends(&f, "");
    press(&mut f, KeyCode::Enter).await;
    press(&mut f, KeyCode::Char('r')).await;
    let bar = |f: &Fixture| {
        crate::tui::test_util::render_lines(120, 30, |frame| {
            f.screen.view(frame, &Theme::default());
        })[29]
            .clone()
    };
    press(&mut f, KeyCode::Char('t')).await;
    let row = bar(&f);
    assert!(
        row.starts_with("Enter accept  Tab complete  Esc cancel"),
        "{row}"
    );
    assert!(!row.contains("y send") && !row.contains("? help"), "{row}");
    press(&mut f, KeyCode::Esc).await;
    let row = bar(&f);
    assert!(row.starts_with("y send  s save to Gmail"), "{row}");
}
