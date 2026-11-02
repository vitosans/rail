//! Saving and opening attachments through the real binary in a pseudo-terminal, against a
//! mocked Gmail: `a` then `s` on one part (AC-44), `w` and `S` on every part of a message, a
//! second save that writes `2-plan.pdf` instead of replacing what is there, and the
//! once-a-session open question that hands a private copy to the system opener and takes it
//! away again. Every wait reads the screen a 100x30 terminal would show — Rail redraws only the
//! cells that changed, so a phrase on the bar is never one run in the stream — and every test
//! checks the files on disk, their modes, and what Gmail was asked for. Linux only, like every
//! pty test.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::bulk;
use super::pty::{screen, script_available, text, wait_until};
use super::{StopsDaemon, WORK_FILE_BACKEND, opened_url_file, write_config};

const API: &str = "/gmail/v1/users/me";
/// `The plan is ready.` in base64, as Gmail serves a text part.
const BODY_DATA: &str = "VGhlIHBsYW4gaXMgcmVhZHku";
/// `%PDF-1.7 plan` in base64url, `plan.pdf`'s thirteen bytes: a PDF's header, because an open
/// checks the bytes against the declared type (#107).
const PLAN_DATA: &str = "JVBERi0xLjcgcGxhbg";
/// `the notes` in base64url, `notes.txt`'s nine.
const NOTES_DATA: &str = "dGhlIG5vdGVz";
/// The attachment list's top border, as `boxed` draws it.
const BOX: &str = "┌ Attachments ";
/// The hint bar, the last row of the 100x30 screen.
const BAR: usize = 29;

/// One attachment of the seeded message, as both the store and fake Gmail describe it.
struct Part {
    /// The MIME part id the store keeps and Gmail's payload carries.
    id: &'static str,
    filename: &'static str,
    mime_type: &'static str,
    /// The bytes Gmail serves, and `data` is those bytes in base64url.
    bytes: &'static [u8],
    data: &'static str,
    /// Gmail's own id for the bytes when they arrive from `attachments.get` instead of inline
    /// in the message. A fetch of that path is a request only a save or an open makes: the
    /// sync reads a message's parts from the message itself.
    attachment_id: Option<&'static str>,
}

/// `plan.pdf`, thirteen bytes inline: AC-44's part.
const PLAN: Part = Part {
    id: "0.1",
    filename: "plan.pdf",
    mime_type: "application/pdf",
    bytes: b"%PDF-1.7 plan",
    data: PLAN_DATA,
    attachment_id: None,
};

/// `notes.txt`, whose bytes Gmail only hands over from `attachments.get`.
const NOTES: Part = Part {
    id: "0.2",
    filename: "notes.txt",
    mime_type: "text/plain",
    bytes: b"the notes",
    data: NOTES_DATA,
    attachment_id: Some("att-notes"),
};

/// `logo.png`, declared `image/png`, whose bytes are an SVG that runs a script when a browser
/// renders it: what a sniffing opener would hand to one (#107).
const DISGUISED: Part = Part {
    id: "0.3",
    filename: "logo.png",
    mime_type: "image/png",
    bytes: b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>",
    data: "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIG9ubG9hZD0iYWxlcnQoMSkiLz4",
    attachment_id: None,
};

impl Part {
    /// The part as Gmail serves it inside the message.
    fn wire(&self) -> Value {
        let size = self.bytes.len();
        let body = match self.attachment_id {
            None => json!({"size": size, "data": self.data}),
            Some(id) => json!({"size": size, "attachmentId": id}),
        };
        json!({
            "partId": self.id, "filename": self.filename,
            "mimeType": self.mime_type, "body": body
        })
    }

    /// The part as the store lists it.
    fn meta(&self) -> rail_core::provider::AttachmentMeta {
        rail_core::provider::AttachmentMeta {
            part_id: self.id.into(),
            filename: self.filename.into(),
            mime_type: self.mime_type.into(),
            size: i64::try_from(self.bytes.len()).unwrap(),
            plain_file: true,
            content_id: None,
        }
    }
}

/// Tokens that stay fresh, so the run never refreshes against the dead token URL.
fn store_fresh_tokens(dir: &tempfile::TempDir) {
    use rail_core::auth::{FileSecretStore, SecretStore, TokenSet};
    let tokens = TokenSet {
        access_token: "test-access".into(),
        refresh_token: "test-refresh".into(),
        expires_at: 4_102_444_800,
    };
    FileSecretStore::new(dir.path().join("data").join("secrets.json"))
        .set_tokens("me@example.com", &tokens)
        .unwrap();
}

/// A store for `WORK_FILE_BACKEND`'s account holding one inbox thread whose body carries
/// `parts`.
fn seed_thread_with_attachments(dir: &tempfile::TempDir, parts: &[Part]) {
    use rail_core::provider::{Body, Envelope, FolderKind, Mailbox, RemoteFolder};
    use rail_core::store::Store;
    use rail_core::sync::Batch;
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let mut store = Store::open(&data.join("rail.db")).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO accounts (name, kind, address) VALUES ('work', 'gmail', 'me@example.com')",
            [],
        )
        .unwrap();
    let envelope = Envelope {
        remote_id: "m1".into(),
        thread_id: "t1".into(),
        subject: "Quarterly plan".into(),
        from: vec![Mailbox {
            name: Some("Tom".into()),
            address: "tom@example.com".into(),
        }],
        date: 1_760_000_000,
        internal_date: 1_760_000_000_000,
        folders: vec!["INBOX".into()],
        ..Envelope::default()
    };
    let body = Body {
        text: "The plan is ready.".into(),
        attachments: parts.iter().map(Part::meta).collect(),
        ..Body::default()
    };
    let batch = Batch {
        folders: Some(vec![RemoteFolder {
            parent: None,
            remote_id: "INBOX".into(),
            name: "Inbox".into(),
            kind: FolderKind::Inbox,
        }]),
        upserts: vec![envelope],
        bodies: vec![("m1".into(), body)],
        ..Batch::new(1)
    };
    store.apply(&batch, 1_760_000_000).unwrap();
}

/// A 200 with a JSON body.
fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

/// The routes the daemon's start-up sync asks for before any thread is opened.
async fn mount_the_sync(server: &MockServer) {
    Mock::given(path(format!("{API}/labels")))
        .respond_with(ok(
            json!({"labels": [{"id": "INBOX", "name": "INBOX", "type": "system"}]}),
        ))
        .mount(server)
        .await;
    Mock::given(path(format!("{API}/profile")))
        .respond_with(ok(
            json!({"emailAddress": "me@example.com", "messagesTotal": 1, "threadsTotal": 1, "historyId": "10"}),
        ))
        .mount(server)
        .await;
    Mock::given(path(format!("{API}/history")))
        .respond_with(ok(json!({"history": [], "historyId": "10"})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/messages")))
        .respond_with(ok(json!({"messages": [{"id": "m1", "threadId": "t1"}]})))
        .mount(server)
        .await;
}

/// Fake Gmail: the start-up sync's routes, plus m1 as a `multipart/mixed` message whose child
/// parts are `parts` — each one's bytes inline, or behind the `attachments.get` path its
/// `attachment_id` names. Opening the thread marks it read, so t1's `modify` is answered too:
/// unanswered, its failure notice could replace a save line before the test read it.
async fn gmail(parts: &[Part]) -> MockServer {
    let server = MockServer::start().await;
    mount_the_sync(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}/threads/t1/modify")))
        .respond_with(ok(json!({"id": "t1"})))
        .mount(&server)
        .await;
    let mut children = vec![json!({
        "partId": "0", "mimeType": "text/plain", "filename": "",
        "body": {"size": 18, "data": BODY_DATA}
    })];
    for part in parts {
        children.push(part.wire());
        if let Some(id) = part.attachment_id {
            Mock::given(path(format!("{API}/messages/m1/attachments/{id}")))
                .respond_with(ok(json!({"size": part.bytes.len(), "data": part.data})))
                .mount(&server)
                .await;
        }
    }
    Mock::given(path(format!("{API}/messages/m1")))
        .respond_with(ok(json!({
            "id": "m1", "threadId": "t1", "labelIds": ["INBOX"], "snippet": "",
            "historyId": "10", "internalDate": "1760000000000", "sizeEstimate": 40,
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "Subject", "value": "Quarterly plan"},
                    {"name": "From", "value": "Tom <tom@example.com>"}
                ],
                "parts": children
            }
        })))
        .mount(&server)
        .await;
    server
}

/// One run of the binary: the live child, its keyboard, the screen it has drawn, the Gmail it
/// talks to and the directory `attachment_dir` points at. The daemon is stopped by the
/// caller's own `StopsDaemon`.
struct Run {
    child: bulk::Spawned,
    keys: ChildStdin,
    output: Arc<Mutex<Vec<u8>>>,
    server: MockServer,
    saved: PathBuf,
}

/// Write the config, the tokens and the thread, start fake Gmail and launch `rail` at 100x30
/// with `attachment_dir` pointing at `<dir>/saved`.
async fn start(dir: &tempfile::TempDir, parts: &[Part]) -> Run {
    let saved = dir.path().join("saved");
    let config = WORK_FILE_BACKEND.replacen(
        "secrets = \"file\"\n",
        &format!(
            "secrets = \"file\"\nattachment_dir = \"{}\"\n",
            saved.display()
        ),
        1,
    );
    write_config(dir, &config);
    store_fresh_tokens(dir);
    seed_thread_with_attachments(dir, parts);
    let server = gmail(parts).await;
    let (child, keys, output) = bulk::launch(dir, &server);
    Run {
        child,
        keys,
        output,
        server,
        saved,
    }
}

/// Wait until the screen, as a 100x30 terminal shows it, passes `check` (rows from 0). The
/// pattern `cli/snooze.rs` follows, and the reason `pty::screen` exists: a redraw touches only
/// the cells that changed, so the stream carries fragments and cursor moves, never the phrase.
fn wait_shown(
    what: &str,
    output: &Arc<Mutex<Vec<u8>>>,
    check: impl Fn(&[String]) -> bool + Send + 'static,
) {
    let drawn = output.clone();
    wait_until(what, output, move || check(&screen(&text(&drawn), 100, 30)));
}

/// Wait until row `row` (from 0) shows `needle`.
fn wait_row(what: &str, output: &Arc<Mutex<Vec<u8>>>, row: usize, needle: String) {
    wait_shown(what, output, move |rows| rows[row].contains(&needle));
}

/// Wait until some row shows `needle`.
fn wait_anywhere(what: &str, output: &Arc<Mutex<Vec<u8>>>, needle: &'static str) {
    wait_shown(what, output, move |rows| {
        rows.iter().any(|r| r.contains(needle))
    });
}

/// Open the thread and wait for the reader. The one barrier a run needs before the attachment
/// keys: the thread's messages come from the daemon, while `a` reads what the reader already
/// holds. Keys after this are written together — the TUI reads them in order, each in the
/// state the one before it left.
fn open_the_thread(run: &mut Run) {
    wait_anywhere("the thread in the list", &run.output, "Quarterly plan");
    bulk::type_keys(&mut run.keys, b"\r");
    wait_anywhere("the reader", &run.output, "ready.");
}

/// `a` in the reader, with the list drawn.
fn open_the_list(run: &mut Run) {
    bulk::type_keys(&mut run.keys, b"a");
    wait_anywhere("the attachment list", &run.output, BOX);
}

/// Send `keys` and wait for Rail to end successfully.
fn ends(run: &mut Run, keys: &[u8]) {
    bulk::type_keys(&mut run.keys, keys);
    let status = bulk::wait_for_exit(&mut run.child, &run.output);
    assert!(status.success(), "{status:?}: {:?}", text(&run.output));
}

/// What `path` holds and the mode it holds it at.
fn saved_file(path: &Path) -> (Vec<u8>, u32) {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    (bytes, mode)
}

/// Every part of `parts` on disk in `dir`, owner-only, holding the bytes Gmail served.
fn each_part_saved(dir: &Path, parts: &[Part]) {
    for part in parts {
        let (bytes, mode) = saved_file(&dir.join(part.filename));
        assert_eq!(bytes, part.bytes, "{}: the served bytes", part.filename);
        assert_eq!(mode, 0o600, "{}: owner-only", part.filename);
    }
}

/// How many times Gmail was asked for `attachment_id`'s bytes.
async fn attachment_fetches(server: &MockServer, attachment_id: &str) -> usize {
    let wanted = format!("{API}/messages/m1/attachments/{attachment_id}");
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == wanted)
        .count()
}

/// How many `format=full` reads of m1 Gmail has answered: a part served inline costs one of
/// these, because Gmail's own attachment id is not stable and Rail re-reads the message.
async fn full_reads(server: &MockServer) -> usize {
    let wanted = format!("{API}/messages/m1");
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| {
            request.url.path() == wanted
                && request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "format" && value == "full")
        })
        .count()
}

/// AC-44 (US-1): `a` then `s` in the reader saves the attachment into `attachment_dir`.
#[tokio::test]
async fn pressing_a_then_s_in_the_reader_saves_the_attachment_to_the_configured_directory() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let mut run = start(&dir, &[PLAN]).await;
    open_the_thread(&mut run);
    // Counted from here, so the read the save makes is told from the sync's own.
    let before = full_reads(&run.server).await;
    open_the_list(&mut run);
    bulk::type_keys(&mut run.keys, b"s");
    // The status line is the barrier, not `file.exists()`: the handler creates the file with
    // `create_new` before it writes and fsyncs it (§7), so a poll between the two would read a
    // short file. The status is printed only after the op answered, and the whole §8 row is
    // waited on over the rebuilt screen.
    wait_row(
        "the save's status line",
        &run.output,
        BAR,
        format!("Saved plan.pdf to {}", run.saved.display()),
    );
    let file = run.saved.join("plan.pdf");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        PLAN.bytes,
        "the served bytes"
    );
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600,
        "owner-only"
    );
    assert_eq!(
        full_reads(&run.server).await,
        before + 1,
        "Gmail was read once more, for the bytes"
    );
    // `s` closes the list and leaves the reader open, where `q` is `reader.back` (a view's own
    // binding beats `Global`), so the first `q` closes the thread and the second quits.
    // Measured on Linux at this anchor: after the first `q` alone the process was still running.
    ends(&mut run, b"qq");
}

/// US-2, FR-9: `w` in the reader saves every attachment of the message under the cursor,
/// without opening the list, and the bar carries §8's save-all line.
#[tokio::test]
async fn w_in_the_reader_saves_every_attachment_of_the_message() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let parts = [PLAN, NOTES];
    let mut run = start(&dir, &parts).await;
    open_the_thread(&mut run);
    bulk::type_keys(&mut run.keys, b"w");
    wait_row(
        "the save-all line",
        &run.output,
        BAR,
        format!("Saved 2 attachments to {}", run.saved.display()),
    );
    each_part_saved(&run.saved, &parts);
    assert_eq!(
        attachment_fetches(&run.server, "att-notes").await,
        1,
        "Gmail served notes.txt's bytes once"
    );
    wait_shown("no list was opened", &run.output, |rows| {
        rows.iter().all(|row| !row.contains(BOX))
    });
    ends(&mut run, b"qq");
}

/// US-2, FR-4: `S` in the attachment list saves every attachment of that row's message and
/// closes the list.
#[tokio::test]
async fn s_capital_in_the_list_saves_every_attachment() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let parts = [PLAN, NOTES];
    let mut run = start(&dir, &parts).await;
    open_the_thread(&mut run);
    open_the_list(&mut run);
    bulk::type_keys(&mut run.keys, b"S");
    wait_row(
        "the save-all line",
        &run.output,
        BAR,
        format!("Saved 2 attachments to {}", run.saved.display()),
    );
    wait_shown("the list closed", &run.output, |rows| {
        rows.iter().all(|row| !row.contains(BOX))
    });
    each_part_saved(&run.saved, &parts);
    assert_eq!(
        attachment_fetches(&run.server, "att-notes").await,
        1,
        "Gmail served notes.txt's bytes once"
    );
    ends(&mut run, b"qq");
}

/// What the two saves left in the destination: the first file still holding the bytes the user
/// put there, the second save's own bytes beside it under the name the bar reported, and nothing
/// else written.
fn assert_the_first_file_was_kept(saved: &Path, first: &Path) {
    assert_eq!(
        std::fs::read(first).unwrap(),
        b"mine",
        "the file that was there"
    );
    let (bytes, mode) = saved_file(&saved.join("2-plan.pdf"));
    assert_eq!(bytes, PLAN.bytes, "the served bytes, beside it");
    assert_eq!(mode, 0o600, "owner-only");
    let mut names: Vec<String> = std::fs::read_dir(saved)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["2-plan.pdf", "plan.pdf"],
        "nothing else was written"
    );
}

/// US-4, FR-11: a second save never replaces the first. `plan.pdf` keeps the bytes it has and
/// the next save lands beside it as `2-plan.pdf`, which is the name the bar reports.
#[tokio::test]
async fn a_second_save_never_replaces_the_first() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let mut run = start(&dir, &[PLAN]).await;
    open_the_thread(&mut run);
    // Counted from here, so the reads the two saves make are told from the sync's own.
    let before = full_reads(&run.server).await;
    open_the_list(&mut run);
    bulk::type_keys(&mut run.keys, b"s");
    let saved = run.saved.clone();
    wait_row(
        "the first save's line",
        &run.output,
        BAR,
        format!("Saved plan.pdf to {}", saved.display()),
    );
    // Bytes of the user's own: the served bytes written twice would tell a replaced file
    // from an untouched one by nothing at all.
    let first = saved.join("plan.pdf");
    std::fs::write(&first, b"mine").unwrap();
    bulk::type_keys(&mut run.keys, b"as");
    wait_row(
        "the second save's line",
        &run.output,
        BAR,
        format!("Saved 2-plan.pdf to {}", saved.display()),
    );
    assert_the_first_file_was_kept(&saved, &first);
    assert_eq!(
        full_reads(&run.server).await,
        before + 2,
        "Gmail was read once per save, for the bytes"
    );
    ends(&mut run, b"qq");
}

/// The file the stand-in opener was handed: the `file://` URL it records in
/// `$RAIL_TEST_URL_FILE`, which it writes by rename, so reading it never catches half a URL.
fn opened_copy(run: &Run, url_file: &Path) -> PathBuf {
    let waiting = url_file.to_path_buf();
    wait_until("the opener's URL", &run.output, move || waiting.is_file());
    let url = std::fs::read_to_string(url_file).unwrap();
    url::Url::parse(url.trim())
        .unwrap_or_else(|error| panic!("{url:?}: {error}"))
        .to_file_path()
        .unwrap_or_else(|()| panic!("{url:?} is not a file URL"))
}

/// The session's first open: `a`, `Enter`, the question, and `y`. Nothing is opened before the
/// answer, and the copy the opener was handed comes back.
fn answer_the_open_question(run: &mut Run, url_file: &Path) -> PathBuf {
    let _ = std::fs::remove_file(url_file);
    open_the_list(run);
    bulk::type_keys(&mut run.keys, b"\r");
    wait_anywhere("the open question", &run.output, "┌ Open this attachment? ");
    assert!(
        !url_file.exists(),
        "the question was asked, so nothing is open yet"
    );
    bulk::type_keys(&mut run.keys, b"y");
    opened_copy(run, url_file)
}

/// `Enter` with the question already answered this session. The question would hold the open
/// back until a key answered it, so a URL arriving at all is the proof it was not asked again.
fn open_without_a_question(run: &mut Run, url_file: &Path) -> PathBuf {
    let _ = std::fs::remove_file(url_file);
    bulk::type_keys(&mut run.keys, b"a\r");
    opened_copy(run, url_file)
}

/// FR-14: the copy is a 0600 file in a 0700 directory of its own, under the attachments
/// directory `RAIL_CACHE_DIR` puts at `<dir>/cache/attachments`.
fn assert_private(copy: &Path, dir: &tempfile::TempDir) {
    let (bytes, mode) = saved_file(copy);
    assert_eq!(bytes, PLAN.bytes, "the served bytes");
    assert_eq!(mode, 0o600, "owner-only");
    let token = copy.parent().expect("a token directory");
    assert_eq!(
        std::fs::metadata(token).unwrap().permissions().mode() & 0o777,
        0o700,
        "the token directory is the owner's alone"
    );
    let attachments = dir.path().join("cache").join("attachments");
    assert_eq!(
        token.parent(),
        Some(attachments.as_path()),
        "under the attachments directory"
    );
}

/// Wait until every copy is gone, and its token directory with it (FR-15).
fn wait_removed(run: &Run, copies: &[&Path]) {
    let going: Vec<PathBuf> = copies.iter().map(|copy| (*copy).to_path_buf()).collect();
    wait_until("the copies to go", &run.output, move || {
        going
            .iter()
            .all(|copy| !copy.exists() && !copy.parent().is_some_and(Path::exists))
    });
}

/// US-3, US-4, FR-5, FR-7, FR-14, FR-15: the session's first `Enter` asks, `y` hands a private
/// copy to the stand-in opener, a second `Enter` opens without asking, and quitting removes
/// both copies and their token directories — closing the reader first leaves them their
/// minute's grace (#108), which a quit does not wait out — and a copy still open when Rail
/// ends goes with it.
#[tokio::test]
async fn opening_asks_once_then_hands_a_private_copy_to_the_opener() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let mut run = start(&dir, &[PLAN]).await;
    let url_file = opened_url_file(&dir);
    open_the_thread(&mut run);
    // Counted from here, so each open's own read is told from the sync's.
    let before = full_reads(&run.server).await;
    let asked = answer_the_open_question(&mut run, &url_file);
    assert_private(&asked, &dir);
    assert_eq!(
        full_reads(&run.server).await,
        before + 1,
        "the first open read Gmail once, for the bytes"
    );
    let again = open_without_a_question(&mut run, &url_file);
    assert_private(&again, &dir);
    assert_eq!(
        full_reads(&run.server).await,
        before + 2,
        "and the second read it once more"
    );
    assert_ne!(again.parent(), asked.parent(), "a token directory each");
    ends(&mut run, b"qq");
    wait_removed(&run, &[&asked, &again]);
    a_copy_still_open_when_rail_ends_goes_with_it().await;
}

/// The session's end, on a run of its own: `q` in the reader is `reader.back`, which starts
/// the copies' grace on its way out, so a copy that is still open when Rail ends needs a quit
/// that skips the reader — Ctrl-C always quits (`tui/app.rs:158`). A second run also starts the
/// session's question again, which `answer_the_open_question` asserts.
async fn a_copy_still_open_when_rail_ends_goes_with_it() {
    let last = tempfile::tempdir().unwrap();
    let _last_daemon = StopsDaemon(&last);
    let mut run = start(&last, &[PLAN]).await;
    let url_file = opened_url_file(&last);
    open_the_thread(&mut run);
    let before = full_reads(&run.server).await;
    let left = answer_the_open_question(&mut run, &url_file);
    assert_eq!(
        full_reads(&run.server).await,
        before + 1,
        "this run's own open read its own Gmail once"
    );
    ends(&mut run, b"\x03");
    assert!(!left.exists(), "quitting left the copy: {}", left.display());
    assert!(
        !left.parent().unwrap().exists(),
        "quitting left its directory: {}",
        left.display()
    );
}

/// #107: a part declared `image/png` whose bytes are an SVG is never handed to the opener —
/// the daemon checks the bytes and refuses, and the TUI saves the part instead and says why.
#[tokio::test]
async fn a_png_whose_bytes_are_svg_is_saved_and_never_opened() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let mut run = start(&dir, &[DISGUISED]).await;
    let url_file = opened_url_file(&dir);
    open_the_thread(&mut run);
    open_the_list(&mut run);
    bulk::type_keys(&mut run.keys, b"\r");
    wait_anywhere("the open question", &run.output, "┌ Open this attachment? ");
    bulk::type_keys(&mut run.keys, b"y");
    wait_anywhere(
        "the refusal's line",
        &run.output,
        "contents do not match its type",
    );
    let (bytes, mode) = saved_file(&run.saved.join("logo.png"));
    assert_eq!(bytes, DISGUISED.bytes, "saved as served");
    assert_eq!(mode, 0o600);
    assert!(!url_file.exists(), "the opener was handed a URL");
    let copies = dir.path().join("cache").join("attachments");
    let left: Vec<_> = std::fs::read_dir(&copies)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "an open copy was written: {left:?}");
    ends(&mut run, b"qq");
}
