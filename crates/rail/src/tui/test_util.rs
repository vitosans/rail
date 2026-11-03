//! Helpers shared by the TUI unit tests.

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::{AccountConfig, AccountKind, Config, Icons, ThreadOrder};
use rail_core::daemon::{
    BeginGate, Connector, DaemonContext, Opener as DaemonOpener, SaveGate, ServeConfig, Server,
    SpawnFn,
};
use rail_core::onboarding::{Event, Wizard};
use rail_core::ops::{AccountSyncStatus, BrowserEnv, OpError, SyncPhase};
use rail_core::paths::Paths;
use rail_core::provider::{Envelope, FolderKind, Mailbox, RemoteFolder};
use rail_core::store::Store;
use rail_core::sync::Batch;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};
use serde_json::json;
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::app::{Clipboard, Effect, Msg, TaskResult, TaskSink};
use super::clock::Clock;
use super::data::{DaemonLink, FakeDaemon, StoreReader};
use super::effects::{EffectContext, Opener};
use super::keys::Keymap;
use super::screen::Screen;
use super::screens::mail::{AccountEntry, MailContext};
use super::screens::wizard::WizardScreen;
use super::theme::Theme;
use super::theme::choice::{ColorDepth, ThemeChoice};

/// Where the in-process test daemons send token refreshes, Gmail and Graph calls: nothing
/// listens.
const UNREACHABLE_TOKEN_URL: &str = "http://127.0.0.1:9/token";
const UNREACHABLE_GMAIL_URL: &str = "http://127.0.0.1:9/gmail/v1/users/me/";
const UNREACHABLE_GRAPH_URL: &str = "http://127.0.0.1:9/v1.0/";

/// The two sizes every rendering test checks: a classic terminal and a roomy one.
pub const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Draw with `draw` on a `width` x `height` test terminal and return the visible rows.
pub fn render_lines(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal.draw(draw).expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// `render_lines` as one snapshot text: each row trimmed of trailing spaces (so an editor that
/// trims them cannot break a snapshot), joined by line breaks.
pub fn snapshot(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    render_lines(width, height, draw)
        .iter()
        .map(|row| row.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `work` with this thread's log lines captured (no colours); return its output and the
/// log text. A second dispatcher kept alive for the whole test process stops a parallel test
/// thread without a subscriber from caching `never` for a callsite.
pub fn logged<R>(work: impl FnOnce() -> R) -> (R, String) {
    static KEEP_TWO: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
    KEEP_TWO.get_or_init(|| tracing::Dispatch::new(tracing_subscriber::registry()));
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || LogSink(sink.clone()))
        .finish();
    let out = tracing::subscriber::with_default(subscriber, work);
    let log = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    (out, log)
}

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Records copies instead of touching the real clipboard.
#[derive(Clone, Default)]
pub struct RecordingClipboard(pub Arc<Mutex<Vec<String>>>);

impl Clipboard for RecordingClipboard {
    fn copy(&mut self, text: &str) -> io::Result<()> {
        self.0
            .lock()
            .expect("clipboard lock")
            .push(text.to_string());
        Ok(())
    }
}

/// A screen that says hello, remembers the keys and copy reports it receives, and answers
/// keys with `on_key`.
pub struct Hello {
    pub typing: bool,
    /// Whether the screen claims every key (see `Screen::claims_key`).
    pub claims: bool,
    pub keys: Vec<KeyEvent>,
    /// Every `TaskResult::Copied` the screen was told about.
    pub copied: Vec<Result<(), String>>,
    /// Every `TaskResult::EditorDone` the screen was told about.
    pub edited: Vec<Result<i32, String>>,
    pub on_key: Box<dyn FnMut(KeyEvent) -> Vec<Effect>>,
    /// Quit before the first frame (see `Screen::start`).
    pub quit_at_start: bool,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            typing: false,
            claims: false,
            keys: Vec::new(),
            copied: Vec::new(),
            edited: Vec::new(),
            on_key: Box::new(|_| Vec::new()),
            quit_at_start: false,
        }
    }
}

impl Screen for Hello {
    fn update(&mut self, msg: Msg) -> Vec<Effect> {
        match msg {
            Msg::Key(key) => {
                self.keys.push(key);
                (self.on_key)(key)
            }
            Msg::Task(TaskResult::Copied(result)) => {
                self.copied.push(result);
                Vec::new()
            }
            Msg::Task(TaskResult::EditorDone(result)) => {
                self.edited.push(result);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn view(&self, frame: &mut Frame, theme: &Theme) {
        frame.render_widget(
            Paragraph::new("hello from rail").style(theme.text),
            frame.area(),
        );
    }

    fn start(&mut self) -> Vec<Effect> {
        if self.quit_at_start {
            vec![Effect::Quit]
        } else {
            Vec::new()
        }
    }

    fn captures_text(&self) -> bool {
        self.typing
    }

    fn claims_key(&self, _key: KeyEvent) -> bool {
        self.claims
    }
}

/// A config with one account, "work" <work@example.com>.
pub fn existing_config() -> Config {
    Config {
        accounts: vec![AccountConfig {
            name: "work".into(),
            kind: AccountKind::Gmail,
            address: "work@example.com".into(),
            color: None,
            label: None,
            client_id: "cid".into(),
            client_secret: None,
            tenant: None,
            signature: None,
            from_name: None,
            ai_send: None,
            history_days: None,
            body_cache_days: None,
        }],
        ..Config::default()
    }
}

/// For screens whose effects are inspected, never run: the paths do not exist, the opener
/// does nothing and there is no daemon.
pub fn test_ctx() -> EffectContext {
    let opener: Opener = Arc::new(|_: &Url| Ok(()));
    let paths = Paths::all_in(Path::new("/nonexistent/rail-test"));
    let spawn: SpawnFn = Arc::new(|| Err(OpError::internal("no daemon in this test")));
    EffectContext::with(opener, Connector::new(paths, spawn))
}

/// A context backed by a real in-process daemon over `paths`. The daemon's opener sends every
/// login and consent URL to the receiver instead of starting a browser; `token_url` replaces
/// the provider's token endpoint.
pub async fn daemon_ctx(
    paths: &Paths,
    token_url: Option<String>,
) -> (EffectContext, mpsc::UnboundedReceiver<Url>) {
    let (ctx, urls, _) = gated_daemon_ctx(paths, token_url).await;
    (ctx, urls)
}

/// The in-process daemon's hooks that hold its begins and saves part way (disarmed).
pub struct Gates {
    pub begin: Arc<BeginGate>,
    pub save: Arc<SaveGate>,
}

/// `daemon_ctx`, with the daemon's gates.
pub async fn gated_daemon_ctx(
    paths: &Paths,
    token_url: Option<String>,
) -> (EffectContext, mpsc::UnboundedReceiver<Url>, Gates) {
    let (tx, rx) = mpsc::unbounded_channel();
    let daemon_opener: DaemonOpener = Arc::new(move |url: &Url, _: &BrowserEnv| {
        let _ = tx.send(url.clone());
        Ok(())
    });
    let gmail_url = token_url.as_ref().map_or_else(
        || UNREACHABLE_GMAIL_URL.to_string(),
        |token| {
            Url::parse(token)
                .unwrap()
                .join("/gmail/v1/users/me/")
                .unwrap()
                .to_string()
        },
    );
    // Never Google or Microsoft: calls go to the fake token/profile server or unreachable addresses.
    let ctx = DaemonContext::new(paths.clone(), daemon_opener)
        .expect("daemon context")
        .with_token_url(Some(
            token_url.unwrap_or_else(|| UNREACHABLE_TOKEN_URL.to_string()),
        ))
        .with_gmail_url(Some(gmail_url))
        .with_graph_url(Some(UNREACHABLE_GRAPH_URL.to_string()));
    let gates = Gates {
        begin: ctx.begin_gate.clone(),
        save: ctx.save_gate.clone(),
    };
    let server = Server::bind(ctx, ServeConfig::default())
        .await
        .expect("bind the test daemon");
    tokio::spawn(server.run());
    let portal: Opener = Arc::new(|_: &Url| Ok(()));
    let spawn: SpawnFn = Arc::new(|| Ok(()));
    let connector = Connector::new(paths.clone(), spawn);
    (EffectContext::with(portal, connector), rx, gates)
}

/// A token endpoint that answers every exchange with `access` and `refresh`.
pub async fn token_server(access: &str, refresh: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access, "refresh_token": refresh, "expires_in": 3600, "token_type": "Bearer"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"emailAddress": "me@example.com"})),
        )
        .mount(&server)
        .await;
    server
}

pub fn query(url: &Url, key: &str) -> String {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .expect("the URL carries the parameter")
}

/// Play the browser for `url`: follow its redirect back with an authorization code.
pub async fn redirect(url: &Url) {
    let redirect = query(url, "redirect_uri");
    reqwest::get(format!(
        "{redirect}/?code=c0de&state={}",
        query(url, "state")
    ))
    .await
    .expect("the loopback listener answered");
}

/// Play the browser for the next URL the daemon opens, waiting at most ten seconds for it.
pub fn redirect_when_asked(mut urls: mpsc::UnboundedReceiver<Url>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let url = tokio::time::timeout(Duration::from_secs(10), urls.recv())
            .await
            .expect("the daemon opened a URL in time")
            .expect("the opener channel is open");
        redirect(&url).await;
    })
}

/// The default keymap, shared.
pub fn keymap() -> Arc<Keymap> {
    Arc::new(Keymap::defaults())
}

pub fn wizard_screen() -> WizardScreen {
    WizardScreen::new(
        Wizard::new(Some(existing_config()), None),
        test_ctx(),
        keymap(),
    )
}

pub fn press(screen: &mut WizardScreen, code: KeyCode) -> Vec<Effect> {
    screen.update(Msg::Key(key(code)))
}

pub fn type_text(screen: &mut WizardScreen, text: &str) {
    for c in text.chars() {
        press(screen, KeyCode::Char(c));
    }
}

pub fn feed(screen: &mut WizardScreen, event: Event) -> Vec<Effect> {
    screen.update(Msg::Task(TaskResult::Wizard(event)))
}

/// From Start to the Gmail login: own app, Gmail, past the instructions, then the address,
/// client ID and secret, and Enter.
pub fn gmail_own_app_keys(address: &str) -> Vec<KeyEvent> {
    let mut keys = vec![
        key(KeyCode::Down),
        key(KeyCode::Enter),
        key(KeyCode::Enter),
        key(KeyCode::Enter),
    ];
    keys.extend(address.chars().map(|c| key(KeyCode::Char(c))));
    keys.extend([key(KeyCode::Tab), key(KeyCode::Tab), key(KeyCode::Tab)]);
    keys.extend("cid".chars().map(|c| key(KeyCode::Char(c))));
    keys.push(key(KeyCode::Tab));
    keys.extend("sec".chars().map(|c| key(KeyCode::Char(c))));
    keys.push(key(KeyCode::Enter));
    keys
}

/// Receive times in fixtures start here (2025-10-09, a fixed point: no test reads the clock).
pub const BASE_MS: i64 = 1_760_000_000_000;

/// A one-message fixture in the inbox, unread, from Sender <sender@example.com>, with
/// subject `subject <id>`.
pub fn mail_envelope(id: &str, thread: &str, ms: i64) -> Envelope {
    Envelope {
        remote_id: id.to_string(),
        thread_id: thread.to_string(),
        subject: format!("subject {id}"),
        from: vec![Mailbox {
            name: Some("Sender".into()),
            address: "sender@example.com".into(),
        }],
        date: ms / 1000,
        internal_date: ms,
        snippet: format!("snippet {id}"),
        folders: vec!["INBOX".into()],
        ..Envelope::default()
    }
}

/// `n` one-message inbox threads `{prefix}t0`.., a minute apart, the last the newest.
pub fn inbox_threads(prefix: &str, n: usize) -> Vec<Envelope> {
    (0..n)
        .map(|i| {
            let ms = BASE_MS + i64::try_from(i).unwrap() * 60_000;
            mail_envelope(&format!("{prefix}m{i}"), &format!("{prefix}t{i}"), ms)
        })
        .collect()
}

/// A migrated store at `paths` with these accounts (name, address) and their Inbox,
/// Projects and Sent folders, holding `envelopes` (each for the account at its index).
pub fn seed_store(
    paths: &Paths,
    accounts: &[(&str, &str)],
    envelopes: &[(usize, Envelope)],
) -> Store {
    let mut store = Store::open(&paths.db_file()).expect("open the store");
    for (name, address) in accounts {
        store
            .conn()
            .execute(
                "INSERT INTO accounts (name, kind, address) VALUES (?1, 'gmail', ?2)",
                [name, address],
            )
            .expect("insert an account");
    }
    let folders = vec![
        RemoteFolder {
            parent: None,
            remote_id: "INBOX".into(),
            name: "Inbox".into(),
            kind: FolderKind::Inbox,
        },
        RemoteFolder {
            parent: None,
            remote_id: "Label_1".into(),
            name: "Projects".into(),
            kind: FolderKind::User,
        },
        RemoteFolder {
            parent: None,
            remote_id: "SENT".into(),
            name: "Sent".into(),
            kind: FolderKind::Sent,
        },
    ];
    for index in 0..accounts.len() {
        let id = i64::try_from(index).unwrap() + 1;
        let upserts = envelopes
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, e)| e.clone())
            .collect();
        let batch = Batch {
            folders: Some(folders.clone()),
            upserts,
            ..Batch::new(id)
        };
        store.apply(&batch, BASE_MS / 1000).expect("seed the store");
    }
    store
}

/// The fixed "now" of every mail-screen test: 2025-10-10 12:40 UTC.
pub const NOW: i64 = 1_760_100_000;

/// An opener that records instead of starting a browser.
pub fn recording_opener() -> (Opener, Arc<Mutex<Vec<Url>>>) {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let log = opened.clone();
    let opener: Opener = Arc::new(move |url: &Url| {
        log.lock().expect("opener log").push(url.clone());
        Ok(())
    });
    (opener, opened)
}

/// A mail context over the store at `paths`, a scripted daemon, a fixed clock in UTC, the
/// default keymap and glyphs, hyperlinks off.
pub fn mail_ctx(paths: &Paths, fake: Arc<FakeDaemon>) -> MailContext {
    MailContext {
        link_actions: Vec::new(),
        paths: paths.clone(),
        keymap: keymap(),
        keymap_preset: rail_core::config::KeymapPreset::Rail,
        store: StoreReader::new(paths.db_file()),
        link: DaemonLink::fake(fake),
        opener: recording_opener().0,
        clock: Clock::fixed(NOW, jiff::tz::TimeZone::UTC),
        icons: Icons::Unicode,
        split_threshold: 140,
        bulk_confirm: 50,
        hints: true,
        show_clock: true,
        thread_order: ThreadOrder::Oldest,
        rich_html: true,
        images: true,
        remote_images: false,
        remote_images_from: Vec::new(),
        picker: Some(ratatui_image::picker::Picker::halfblocks()),
        dim_read: rail_core::config::DimRead::Dot,
        field_keys: rail_core::config::FieldKeys::Readline,
        hyperlinks: false,
        depth: ColorDepth::TrueColor,
        theme_choice: ThemeChoice::default(),
        editor: vec!["fake".into()],
        signatures: vec![Some(Ok("work sig".into())), None],
        attachment_dir: Some(paths.cache_dir.join("saved")),
        saved_searches: vec![],
        saved_search_counts: true,
    }
}

/// `work` <w@example.com> (cyan) and `home` <h@example.com> (magenta), in that order.
pub fn work_and_home() -> Vec<AccountEntry> {
    vec![
        AccountEntry {
            name: "work".into(),
            label: "work".into(),
            address: "w@example.com".into(),
            kind: AccountKind::Gmail,
            color: Some(Color::Cyan),
        },
        AccountEntry {
            name: "home".into(),
            label: "home".into(),
            address: "h@example.com".into(),
            kind: AccountKind::Gmail,
            color: Some(Color::Magenta),
        },
    ]
}

pub fn status(name: &str, address: &str, phase: SyncPhase) -> AccountSyncStatus {
    AccountSyncStatus {
        phase,
        last_success_at: Some(NOW - 60),
        ..AccountSyncStatus::new(name, address, AccountKind::Gmail)
    }
}

/// A stream factory that hands out `stream` once (for `run_loop` tests that run no editor).
pub fn once<E>(stream: E) -> impl FnMut() -> E {
    let mut slot = Some(stream);
    move || slot.take().expect("the test stream was made once")
}

/// What a fake editor does to the draft file and what it exits with.
type EditorHook = Box<dyn FnMut(&Path) -> Result<i32, String> + Send>;

/// The fake editor `settle_capturing` runs for an `Effect::RunEditor`: a test sets the hook,
/// which may rewrite the file; unset, the editor leaves the file alone and exits 0.
#[derive(Clone, Default)]
pub struct Editor(Arc<Mutex<Option<EditorHook>>>);

impl Editor {
    /// Run `hook` for each `Effect::RunEditor` from now on.
    pub fn set(&self, hook: impl FnMut(&Path) -> Result<i32, String> + Send + 'static) {
        *self.0.lock().unwrap() = Some(Box::new(hook));
    }

    fn run(&self, path: &Path) -> Result<i32, String> {
        match self.0.lock().unwrap().as_mut() {
            Some(hook) => hook(path),
            None => Ok(0),
        }
    }
}

/// Run every job `effects` start, feeding what each reports back to `screen`, until nothing
/// is left. Each job must finish (the fake daemon's always do); timers really sleep.
pub async fn settle<S: Screen>(screen: &mut S, effects: Vec<Effect>) {
    settle_capturing(
        screen,
        effects,
        &Arc::new(Mutex::new(None)),
        &Editor::default(),
    )
    .await;
}

/// `settle`, keeping the last `Effect::Theme` seen in `themes` and answering each
/// `Effect::RunEditor` with `editor` (as `TaskResult::EditorDone`).
pub async fn settle_capturing<S: Screen>(
    screen: &mut S,
    effects: Vec<Effect>,
    themes: &Arc<Mutex<Option<Theme>>>,
    editor: &Editor,
) {
    let mut queue: VecDeque<Effect> = effects.into();
    let mut ran = 0;
    while let Some(effect) = queue.pop_front() {
        ran += 1;
        assert!(ran < 1_000, "the effects never settled");
        let job = match effect {
            Effect::Spawn(job) => job,
            Effect::Theme(theme) => {
                *themes.lock().unwrap() = Some(theme);
                continue;
            }
            Effect::RunEditor { path, .. } => {
                let result = editor.run(&path);
                queue.extend(screen.update(Msg::Task(TaskResult::EditorDone(result))));
                continue;
            }
            _ => continue,
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::time::timeout(Duration::from_secs(10), job(TaskSink::new(tx, 0)))
            .await
            .expect("a job finished in time");
        while let Ok((_, result)) = rx.try_recv() {
            queue.extend(screen.update(Msg::Task(result)));
        }
    }
}

/// Make `config.toml` unwritable for any user, root too: its directory is a regular file, so
/// reading and replacing it both fail.
pub fn block_config(paths: &Paths) {
    let dir = paths.config_dir.clone();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::write(&dir, "not a directory").unwrap();
}
