//! The real binary in a pseudo-terminal, through util-linux `script`. Review focus 5, end to
//! end: the wizard reaches the login wait, gets Ctrl-C (byte 0x03, a key in raw mode), restores
//! the terminal and saves nothing. And `rail account remove` asks, and removes only on a yes.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    StopsDaemon, WORK_FILE_BACKEND, rail_env, store_work_tokens, write_config, write_opener,
};

const FILE_BACKEND: &str = "[general]\nsecrets = \"file\"\n";
const ACME: &str = "name = \"Acme Gmail\"\nkind = \"gmail\"\nclient_id = \"acme.apps.googleusercontent.com\"\nclient_secret = \"GOCSPX-acme\"\n";
/// Bound on every wait; the child is killed when the test ends either way.
pub(super) const LIMIT: Duration = Duration::from_secs(30);

/// Kills the child if the test fails half way, so no wizard is left running.
pub(super) struct Running(pub(super) Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn script_available() -> bool {
    Command::new("script")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Everything the pseudo-terminal shows, collected on a thread.
pub(super) fn collect(mut out: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = buffer.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = out.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });
    buffer
}

pub(super) fn text(buffer: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap()).into_owned()
}

/// Poll `done` until it holds; on timeout fail with what the terminal showed.
pub(super) fn wait_until(what: &str, output: &Mutex<Vec<u8>>, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !done() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen:\n{:?}",
            text(output)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Start `rail setup` under `script` with a preset available and an opener that never
/// redirects back.
fn start(dir: &tempfile::TempDir, url_file: &std::path::Path) -> (Running, ChildStdin) {
    let presets = dir.path().join("config").join("presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(presets.join("acme.toml"), ACME).unwrap();
    let command = format!(
        "stty cols 100 rows 30; exec '{}' setup",
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, dir)
        .env("RAIL_TEST_OPENER", write_opener(dir))
        .env("RAIL_TEST_URL_FILE", url_file)
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let keys = child.0.stdin.take().unwrap();
    (child, keys)
}

pub(super) fn wait_for_exit(child: &mut Running, output: &Mutex<Vec<u8>>) -> ExitStatus {
    let deadline = Instant::now() + LIMIT;
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "rail did not quit:\n{:?}",
            text(output)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn ctrl_c_during_login_wait_restores_the_terminal_and_saves_nothing() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&dir, FILE_BACKEND);
    let url_file = dir.path().join("url.txt");
    let (mut child, mut keys) = start(&dir, &url_file);
    let output = collect(child.0.stdout.take().unwrap());
    let seen = |needle: &'static str| {
        let output = output.clone();
        move || text(&output).contains(needle)
    };

    wait_until("the start step", &output, seen("Use a preset"));
    keys.write_all(b"\r").unwrap();
    wait_until("the preset list", &output, seen("Acme Gmail"));
    keys.write_all(b"\r").unwrap();
    wait_until("the details step", &output, seen("Email address"));
    keys.write_all(b"me@example.com\r").unwrap();
    wait_until("the browser to be asked", &output, || url_file.exists());
    keys.write_all(&[0x03]).unwrap();
    keys.flush().unwrap();

    let status = wait_for_exit(&mut child, &output);
    drop(keys);
    std::thread::sleep(Duration::from_millis(200));
    let out = text(&output);
    let entered = out
        .find("\x1b[?1049h")
        .expect("entered the alternate screen");
    let left = out.rfind("\x1b[?1049l").expect("left the alternate screen");
    assert!(left > entered, "{out:?}");
    assert!(out[left..].contains("setup cancelled"), "{out:?}");
    assert_eq!(status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(config).unwrap(), FILE_BACKEND);
    assert!(
        !dir.path().join("data").join("secrets.json").exists(),
        "no tokens"
    );
    assert!(
        !dir.path().join("data").join("rail.db").exists(),
        "no store row"
    );
}
/// Start `rail <args>` under `script`, so stdin and stdout are a terminal. `args` is shell
/// text, quoted by the caller.
pub(super) fn start_in_terminal(dir: &tempfile::TempDir, args: &str) -> (Running, ChildStdin) {
    let command = format!("exec '{}' {args}", env!("CARGO_BIN_EXE_rail"));
    let mut script = Command::new("script");
    rail_env(&mut script, dir)
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let keys = child.0.stdin.take().unwrap();
    (child, keys)
}

/// Answer the removal prompt with `answer`; returns the exit status and the screen.
fn answer_removal(dir: &tempfile::TempDir, answer: &[u8]) -> (ExitStatus, String) {
    answer_removal_after(dir, answer, || {})
}

/// `answer_removal`, running `meanwhile` while the question is on the screen.
fn answer_removal_after(
    dir: &tempfile::TempDir,
    answer: &[u8],
    meanwhile: impl FnOnce(),
) -> (ExitStatus, String) {
    answer_in_terminal(
        dir,
        "account remove 'ME@example.com'",
        "Remove? [y/N]",
        answer,
        meanwhile,
    )
}

/// Run `rail <args>` in a terminal, wait for `prompt`, run `meanwhile`, type `answer`; returns
/// the exit status and the screen.
fn answer_in_terminal(
    dir: &tempfile::TempDir,
    args: &str,
    prompt: &'static str,
    answer: &[u8],
    meanwhile: impl FnOnce(),
) -> (ExitStatus, String) {
    let (mut child, mut keys) = start_in_terminal(dir, args);
    let output = collect(child.0.stdout.take().unwrap());
    let shown = output.clone();
    wait_until("the prompt", &output, move || text(&shown).contains(prompt));
    meanwhile();
    keys.write_all(answer).unwrap();
    keys.flush().unwrap();
    let status = wait_for_exit(&mut child, &output);
    drop(keys);
    std::thread::sleep(Duration::from_millis(200));
    (status, text(&output))
}

#[test]
fn account_remove_on_a_terminal_removes_after_yes() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let secrets = store_work_tokens(&dir);
    let (status, screen) = answer_removal(&dir, b"Yes\r");
    assert_eq!(status.code(), Some(0), "{screen:?}");
    assert!(
        screen.contains("Remove account \"work\" (\"me@example.com\", gmail)?"),
        "{screen:?}"
    );
    assert!(screen.contains("Mail already synced stays"), "{screen:?}");
    assert!(
        screen.contains("Removed \"work\" <me@example.com>"),
        "{screen:?}"
    );
    let left = std::fs::read_to_string(&config).unwrap();
    assert!(!left.contains("me@example.com"), "{left}");
    assert!(left.contains("secrets = \"file\""), "{left}");
    let tokens = std::fs::read_to_string(&secrets).unwrap();
    assert!(!tokens.contains("me@example.com"), "{tokens}");
}

/// The account changes while the question is on the screen; the yes was for the
/// old account, so nothing is removed and the user is told to run the command again.
#[test]
fn account_remove_after_the_account_changed_removes_nothing() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let changed = WORK_FILE_BACKEND.replace("kind = \"gmail\"", "kind = \"m365\"");
    let edit = changed.clone();
    let path = config.clone();
    let (status, screen) =
        answer_removal_after(&dir, b"y\r", move || std::fs::write(&path, edit).unwrap());
    assert_eq!(status.code(), Some(1), "{screen:?}");
    assert!(
        screen.contains("the account changed while you answered; nothing was removed"),
        "{screen:?}"
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), changed);
}

#[test]
fn account_remove_on_a_terminal_keeps_everything_after_no() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, WORK_FILE_BACKEND);
    let secrets = store_work_tokens(&dir);
    let before = std::fs::read_to_string(&secrets).unwrap();
    let (status, screen) = answer_removal(&dir, b"\r");
    assert_eq!(status.code(), Some(1), "{screen:?}");
    assert!(screen.contains("nothing was removed"), "{screen:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), WORK_FILE_BACKEND);
    assert_eq!(std::fs::read_to_string(&secrets).unwrap(), before);
}

/// No account has tokens, so the keyring is never asked: only the config changes.
const KEYRING_BACKEND: &str = "[general]\nsecrets = \"keyring\"\n";

/// Final review F4: `rail secrets set` shows the daemon's text, asks, and changes the setting
/// on a yes.
#[test]
fn secrets_set_on_a_terminal_changes_the_store_after_yes() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, KEYRING_BACKEND);
    let (status, screen) =
        answer_in_terminal(&dir, "secrets set file", "Change? [y/N]", b"y\r", || {});
    assert_eq!(status.code(), Some(0), "{screen:?}");
    assert!(
        screen.contains("from keyring to file") && screen.contains("plain 0600 file"),
        "{screen:?}"
    );
    assert!(
        screen.contains("Tokens now go to the file backend"),
        "{screen:?}"
    );
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("secrets = \"file\"")
    );
}

/// An empty answer is a no: nothing changes.
#[test]
fn secrets_set_on_a_terminal_changes_nothing_after_an_empty_answer() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, KEYRING_BACKEND);
    let (status, screen) =
        answer_in_terminal(&dir, "secrets set file", "Change? [y/N]", b"\r", || {});
    assert_eq!(status.code(), Some(1), "{screen:?}");
    assert!(screen.contains("nothing changed"), "{screen:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), KEYRING_BACKEND);
}

/// The settings change while the question is on the screen: the yes was for the old plan.
#[test]
fn secrets_set_after_the_settings_changed_changes_nothing() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, KEYRING_BACKEND);
    // A vault setting enters the plan; no account means the keyring is still never asked.
    let edited = format!("{KEYRING_BACKEND}\n[onepassword]\nvault = \"Other\"\n");
    let (path, edit) = (config.clone(), edited.clone());
    let (status, screen) = answer_in_terminal(
        &dir,
        "secrets set file",
        "Change? [y/N]",
        b"y\r",
        move || std::fs::write(&path, edit).unwrap(),
    );
    assert_eq!(status.code(), Some(1), "{screen:?}");
    assert!(
        screen.contains("the settings changed while you answered; nothing changed — run it again"),
        "{screen:?}"
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), edited);
}

/// A setting already in place needs no question.
#[test]
fn secrets_set_to_the_current_store_says_nothing_needed_changing() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    let config = write_config(&dir, FILE_BACKEND);
    let (mut child, keys) = start_in_terminal(&dir, "secrets set file");
    let output = collect(child.0.stdout.take().unwrap());
    let status = wait_for_exit(&mut child, &output);
    drop(keys);
    std::thread::sleep(Duration::from_millis(200));
    let screen = text(&output);
    assert_eq!(status.code(), Some(0), "{screen:?}");
    assert!(screen.contains("already in place"), "{screen:?}");
    assert!(!screen.contains("Change? [y/N]"), "{screen:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), FILE_BACKEND);
}

/// Spec §7: a panic in the mail screen leaves the user's terminal restored, then prints.
#[test]
fn a_panic_in_the_mail_screen_restores_the_terminal() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    let command = format!(
        "stty cols 100 rows 30; exec '{}'",
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, &dir)
        .env("RAIL_TEST_MAIL_PANIC", "1")
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let mut keys = child.0.stdin.take().unwrap();
    let output = collect(child.0.stdout.take().unwrap());
    let shown = output.clone();
    wait_until("the mail screen", &output, move || {
        text(&shown).contains("All inboxes")
    });
    keys.write_all(b"x").unwrap();
    keys.flush().unwrap();
    let status = wait_for_exit(&mut child, &output);
    drop(keys);
    std::thread::sleep(Duration::from_millis(200));
    let out = text(&output);
    assert!(!status.success());
    let entered = out
        .find("\x1b[?1049h")
        .expect("entered the alternate screen");
    let left = out.rfind("\x1b[?1049l").expect("left the alternate screen");
    let panicked = out
        .rfind("rail test panic in the mail screen")
        .expect("the panic message");
    assert!(entered < left && left < panicked, "{out:?}");
}

/// Start `rail` under `script` and wait for `wanted` on the screen, then quit with `q`.
pub(super) fn mail_screen_shows(
    dir: &tempfile::TempDir,
    wanted: &str,
    tweak: impl FnOnce(&mut Command),
) {
    mail_screen_draws(dir, wanted, tweak, |shown| shown.contains(wanted));
}

/// Start the mail screen at 100x30, wait until `done` holds for what the terminal showed (`what`
/// names it in a timeout), then quit.
pub(super) fn mail_screen_draws(
    dir: &tempfile::TempDir,
    what: &str,
    tweak: impl FnOnce(&mut Command),
    done: impl Fn(&str) -> bool,
) {
    let command = format!(
        "stty cols 100 rows 30; exec '{}'",
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, dir)
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    tweak(&mut script);
    let mut child = Running(script.spawn().unwrap());
    let mut keys = child.0.stdin.take().unwrap();
    let output = collect(child.0.stdout.take().unwrap());
    let shown = output.clone();
    wait_until(what, &output, move || done(&text(&shown)));
    keys.write_all(b"q").unwrap();
    keys.flush().unwrap();
    wait_for_exit(&mut child, &output);
}

/// The screen row (1 is the first) `needle` was last drawn on: the row of the last cursor move
/// (`ESC [ row ; col H`) before it. The terminal backend moves the cursor only between cells
/// that are not side by side, so the move before a run of text is on that text's row.
/// The screen a `cols` x `rows` terminal shows after `shown`, one string per row. Rail draws only
/// the cells that change, so a word on screen need not be one run in the stream: `Snoozed` over
/// a bar that had an `e` in the same column arrives as `Snooz`, a cursor move, `d`. Cursor moves
/// (`CSI row;col H`), a full clear and printable cells are applied; every other escape is skipped.
pub(super) fn screen(shown: &str, cols: usize, rows: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let mut grid = vec![vec![' '; cols]; rows];
    let (mut row, mut col) = (0usize, 0usize);
    let mut chars = shown.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next_if_eq(&'[').is_none() {
                chars.next();
                continue;
            }
            let mut params = String::new();
            let mut last = None;
            for n in chars.by_ref() {
                if ('@'..='~').contains(&n) {
                    last = Some(n);
                    break;
                }
                params.push(n);
            }
            match last {
                Some('H') => {
                    let mut at = params.split(';').map(|p| p.parse::<usize>().unwrap_or(1));
                    row = at.next().unwrap_or(1).saturating_sub(1);
                    col = at.next().unwrap_or(1).saturating_sub(1);
                }
                Some('J') if params == "2" => grid.iter_mut().for_each(|r| r.fill(' ')),
                _ => {}
            }
            continue;
        }
        let width = c.width().unwrap_or(0);
        if width == 0 {
            continue;
        }
        if let Some(cells) = grid.get_mut(row) {
            if col < cols {
                cells[col] = c;
            }
            // The second cell of a wide character is left out when the row is read.
            if width == 2 && col + 1 < cols {
                cells[col + 1] = '\0';
            }
        }
        col += width;
    }
    grid.into_iter()
        .map(|cells| {
            let line: String = cells.into_iter().filter(|c| *c != '\0').collect();
            line.trim_end().to_string()
        })
        .collect()
}

pub(super) fn drawn_row(shown: &str, needle: &str) -> Option<u16> {
    let before = &shown[..shown.rfind(needle)?];
    before.rmatch_indices("\x1b[").find_map(|(at, _)| {
        let rest = &before[at + 2..];
        let (pos, _) = rest.split_once('H')?;
        let (row, col) = pos.split_once(';')?;
        col.parse::<u16>().ok()?;
        row.parse().ok()
    })
}

#[test]
fn a_store_that_is_not_set_up_shows_the_hint_in_the_list_area() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    mail_screen_shows(&dir, "Run `rail sync`", |_| {});
}

#[test]
fn a_daemon_that_cannot_be_reached_shows_in_the_header() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    // A runtime directory under a regular file can never be created, so no daemon can start.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    // The header is the first row; the hint bar (the last) shows only the accounts.
    let wanted = "daemon unreachable";
    let tweak = |cmd: &mut Command| {
        cmd.env("RAIL_RUNTIME_DIR", blocker.join("run"));
    };
    mail_screen_draws(&dir, "daemon unreachable in the header", tweak, |shown| {
        drawn_row(shown, wanted) == Some(1)
    });
}

/// A store for `WORK_FILE_BACKEND`'s account holding one inbox thread whose body has HTML.
fn seed_html_thread(dir: &tempfile::TempDir) {
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
        html: Some("<p>The plan is ready.</p>".into()),
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

/// Closing the terminal window or `kill` ends Rail as `q` does: the file `o` wrote is gone
/// and the terminal is restored (spec §6: "deleted on close").
#[test]
fn sigterm_with_a_file_open_removes_it_and_restores_the_terminal() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    seed_html_thread(&dir);
    let pid_file = dir.path().join("rail.pid");
    let url_file = dir.path().join("url.txt");
    // After `exec` the shell's pid is Rail's.
    let command = format!(
        "stty cols 100 rows 30; echo $$ > '{}'; exec '{}'",
        pid_file.display(),
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, &dir)
        .env("RAIL_TEST_URL_FILE", &url_file)
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let mut keys = child.0.stdin.take().unwrap();
    let output = collect(child.0.stdout.take().unwrap());
    let seen = |needle: &'static str| {
        let output = output.clone();
        move || text(&output).contains(needle)
    };
    wait_until("the thread in the list", &output, seen("Quarterly plan"));
    keys.write_all(b"\r").unwrap();
    keys.flush().unwrap();
    wait_until("the reader", &output, seen("ready."));
    keys.write_all(b"o").unwrap();
    keys.flush().unwrap();
    wait_until("the confirmation", &output, seen("Remote"));
    keys.write_all(b"y").unwrap();
    keys.flush().unwrap();
    wait_until("the browser to be asked", &output, || url_file.exists());
    let open_dir = dir.path().join("cache").join("open");
    let files = || std::fs::read_dir(&open_dir).map_or(0, Iterator::count);
    assert_eq!(files(), 1, "the file `o` wrote");

    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let killed = Command::new("kill")
        .args(["-TERM", pid.trim()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = wait_for_exit(&mut child, &output);
    drop(keys);
    std::thread::sleep(Duration::from_millis(200));
    let out = text(&output);
    assert!(status.success(), "{status:?}: {out:?}");
    assert_eq!(files(), 0, "the file went with Rail");
    let entered = out
        .find("\x1b[?1049h")
        .expect("entered the alternate screen");
    let left = out.rfind("\x1b[?1049l").expect("left the alternate screen");
    assert!(entered < left, "{out:?}");
}

/// Best effort: kills the process whose pid is in the file, so a failing test never leaves
/// Rail lingering behind its pty.
struct KillsRail(Option<std::path::PathBuf>);

impl KillsRail {
    fn new(pid_file: std::path::PathBuf) -> Self {
        Self(Some(pid_file))
    }

    /// Rail exited: leave its (possibly reused) pid alone.
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for KillsRail {
    fn drop(&mut self) {
        let Some(pid_file) = self.0.take() else {
            return;
        };
        if let Ok(pid) = std::fs::read_to_string(pid_file) {
            let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
        }
    }
}

/// A signal after the run loop ended (the wizard's settle, the final report) ends Rail as it
/// ends any program: no run loop is listening any more.
#[test]
fn sigterm_after_the_mail_screen_closed_ends_rail() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    let pid_file = dir.path().join("rail.pid");
    let mut kills_rail = KillsRail::new(pid_file.clone());
    // After `exec` the shell's pid is Rail's.
    let command = format!(
        "stty cols 100 rows 30; echo $$ > '{}'; exec '{}'",
        pid_file.display(),
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, &dir)
        // Far longer than LIMIT: only the signal can end Rail in time.
        .env("RAIL_TEST_LINGER_SECS", "120")
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let mut keys = child.0.stdin.take().unwrap();
    let output = collect(child.0.stdout.take().unwrap());
    let shown = output.clone();
    wait_until("the mail screen", &output, move || {
        text(&shown).contains("All inboxes")
    });
    keys.write_all(b"q").unwrap();
    keys.flush().unwrap();
    let restored = output.clone();
    wait_until("the terminal restored", &output, move || {
        text(&restored).contains("\x1b[?1049l")
    });
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let killed = Command::new("kill")
        .args(["-TERM", pid.trim()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = wait_for_exit(&mut child, &output);
    // Rail has exited: its pid may be reused, so the guard must not kill it now.
    kills_rail.disarm();
    drop(keys);
    assert!(
        !status.success(),
        "ended by the signal, not by a clean exit: {status:?}"
    );
}

/// #155: a terminal that never answers the picture query (here `script`'s pseudo-terminal, as a
/// tmux session with no client) once left the query's reader behind, taking every key, and
/// later turned raw mode off. With the query on and a real `TERM`, `?` opens help and `q`
/// quits.
#[test]
fn keys_reach_the_mail_screen_when_the_terminal_never_answers_the_picture_query() {
    if !script_available() {
        eprintln!("skipping: util-linux `script` is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _daemon = StopsDaemon(&dir);
    write_config(&dir, WORK_FILE_BACKEND);
    let command = format!(
        "stty cols 100 rows 30; exec '{}'",
        env!("CARGO_BIN_EXE_rail")
    );
    let mut script = Command::new("script");
    rail_env(&mut script, &dir)
        .env_remove("RAIL_TEST_NO_IMAGE_QUERY")
        .env("TERM", "xterm-256color")
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Running(script.spawn().unwrap());
    let mut keys = child.0.stdin.take().unwrap();
    let output = collect(child.0.stdout.take().unwrap());
    let shown = output.clone();
    wait_until("the mail screen", &output, move || {
        text(&shown).contains("Run `rail sync`")
    });
    keys.write_all(b"?").unwrap();
    keys.flush().unwrap();
    let shown = output.clone();
    wait_until("help after ?", &output, move || {
        text(&shown).contains("Show the keys for this view")
    });
    keys.write_all(b"\x1bq").unwrap();
    keys.flush().unwrap();
    wait_for_exit(&mut child, &output);
}
