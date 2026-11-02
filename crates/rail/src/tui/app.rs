//! The shell's model (the active screen, the theme and the quit flag), the messages that
//! drive it, and the effects it asks the event loop to run.

use std::io;
use std::sync::Arc;

use crossterm::clipboard::CopyToClipboard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use futures_util::future::BoxFuture;
use ratatui::Frame;
use ratatui::style::Style;
use tokio::sync::mpsc;

use super::keys::{Action, Chord, Keymap, Resolved, View};
use super::screen::Screen;
use super::theme::Theme;

/// Everything the shell reacts to.
#[derive(Debug)]
pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    /// Every 250 ms, so spinners can animate.
    Tick,
    Task(TaskResult),
}

/// What a background task, or the event loop on its behalf, reports back to the screen.
#[derive(Debug)]
pub enum TaskResult {
    /// An onboarding wizard effect finished or made progress.
    Wizard(rail_core::onboarding::Event),
    /// A spawned job panicked; carries the panic message when it was a string.
    JobPanicked(String),
    /// The outcome of an `Effect::Copy`: `Err` holds the reason it could not copy.
    Copied(Result<(), String>),
    /// A mail-screen query or daemon call answered (see `tui::data`).
    Mail(crate::tui::data::MailEvent),
    /// The editor `Effect::RunEditor` ran has exited: its exit code, or why it could not run.
    EditorDone(Result<i32, String>),
}

/// A result tagged with the generation of the job that sent it (see `Tasks`).
pub type TaggedResult = (u64, TaskResult);

/// Where a running task sends its results. Every result carries the generation the job was
/// spawned in, so the event loop can drop results that outlived a cancel.
#[derive(Debug, Clone)]
pub struct TaskSink {
    tx: mpsc::UnboundedSender<TaggedResult>,
    generation: u64,
}

impl TaskSink {
    /// A sink that tags results with `generation`.
    pub fn new(tx: mpsc::UnboundedSender<TaggedResult>, generation: u64) -> Self {
        Self { tx, generation }
    }

    /// Send `result` to the event loop. Fails only when the loop is gone.
    #[expect(
        clippy::result_large_err,
        reason = "mpsc hands the unsent result back; callers only ever drop it"
    )]
    pub fn send(&self, result: TaskResult) -> Result<(), mpsc::error::SendError<TaskResult>> {
        self.tx
            .send((self.generation, result))
            .map_err(|mpsc::error::SendError((_, result))| mpsc::error::SendError(result))
    }
}

/// A unit of background work. The event loop starts it and aborts it when it is cancelled.
pub type Job = Box<dyn FnOnce(TaskSink) -> BoxFuture<'static, ()> + Send>;

/// What `update` asks the event loop to do.
pub enum Effect {
    Quit,
    /// Abort every task still running.
    CancelAll,
    Spawn(Job),
    /// Put text on the system clipboard, then report `TaskResult::Copied`.
    Copy(String),
    /// Replace the theme (a picker's preview, choice or cancel).
    Theme(Theme),
    /// Replace the keymap the shell reads the quit key with (`:keymap`).
    Keymap(Arc<Keymap>),
    /// Leave the terminal, run `command` on `path` until it exits, take the terminal back,
    /// then report `TaskResult::EditorDone`. Nothing else runs meanwhile.
    RunEditor {
        command: Vec<String>,
        path: std::path::PathBuf,
    },
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quit => f.write_str("Quit"),
            Self::CancelAll => f.write_str("CancelAll"),
            Self::Spawn(_) => f.write_str("Spawn(..)"),
            Self::Copy(_) => f.write_str("Copy(..)"),
            Self::Theme(_) => f.write_str("Theme(..)"),
            Self::Keymap(_) => f.write_str("Keymap(..)"),
            Self::RunEditor { .. } => f.write_str("RunEditor(..)"),
        }
    }
}

/// The system clipboard, behind a trait so tests can record instead.
pub trait Clipboard {
    /// Put `text` on the clipboard.
    fn copy(&mut self, text: &str) -> io::Result<()>;
}

/// Copies through the terminal with an OSC 52 sequence, which also works over SSH. The event
/// loop writes it between frames; it is a control sequence, never text on the screen.
pub struct Osc52;

impl Clipboard for Osc52 {
    fn copy(&mut self, text: &str) -> io::Result<()> {
        execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text))
    }
}

/// The shell's state: the active screen plus what is shared across screens.
pub struct App<S> {
    pub screen: S,
    pub theme: Theme,
    pub quit: bool,
    pub clipboard: Box<dyn Clipboard>,
    /// Every action's keys, built from `config.toml` at startup.
    pub keymap: Arc<Keymap>,
}

impl<S: Screen> App<S> {
    /// A shell showing `screen`, with the default theme.
    pub fn new(screen: S, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            screen,
            theme: Theme::default(),
            quit: false,
            clipboard,
            keymap: Arc::new(Keymap::defaults()),
        }
    }

    /// The shell with `keymap` (built from `config.toml` at startup).
    pub fn with_keymap(mut self, keymap: Arc<Keymap>) -> Self {
        self.keymap = keymap;
        self
    }

    /// The shell starting in `theme`.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

/// `Ctrl-C` always quits; the quit key (`global.quit`, `q` by default) quits unless the screen
/// is taking text or binds that key itself. While the screen defers quitting, both go to the
/// screen instead. Everything else goes to the screen.
pub fn update<S: Screen>(app: &mut App<S>, msg: Msg) -> Vec<Effect> {
    if let Msg::Key(key) = &msg
        && !app.screen.defers_quit()
        && quits(&app.keymap, *key, &app.screen)
    {
        app.quit = true;
        return vec![Effect::Quit];
    }
    if let Msg::Resize(width, height) = &msg {
        tracing::trace!(width, height, "terminal resized");
    }
    app.screen.update(msg)
}

fn quits<S: Screen>(keymap: &Keymap, key: KeyEvent, screen: &S) -> bool {
    let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl_c {
        return true;
    }
    if screen.captures_text() || screen.claims_key(key) {
        return false;
    }
    let mut chord = Chord::default();
    keymap.resolve(&[View::Global], &mut chord, key) == Resolved::Action(Action::GlobalQuit)
}

/// Draw the active screen.
pub fn view<S: Screen>(app: &App<S>, frame: &mut Frame) {
    if let Some(background) = app.theme.background {
        let fill = ratatui::widgets::Block::new().style(Style::new().bg(background));
        frame.render_widget(fill, frame.area());
    }
    app.screen.view(frame, &app.theme);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_util::{Hello, RecordingClipboard, SIZES, ctrl, key, render_lines};
    use ratatui::style::Color;

    #[test]
    fn view_fills_the_background_when_the_theme_has_one() {
        let mut app = app(false);
        app.theme.background = Some(Color::Rgb(1, 2, 3));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 4)).unwrap();
        terminal.draw(|frame| view(&app, frame)).unwrap();
        let cell = &terminal.backend().buffer()[(19, 3)];
        assert_eq!(cell.bg, Color::Rgb(1, 2, 3), "the last cell is painted");
    }

    fn app(typing: bool) -> App<Hello> {
        let screen = Hello {
            typing,
            ..Hello::default()
        };
        App::new(screen, Box::new(RecordingClipboard::default()))
    }

    #[test]
    fn ctrl_c_quits_even_while_typing() {
        let mut app = app(true);
        let effects = update(&mut app, Msg::Key(ctrl('c')));
        assert!(app.quit);
        assert!(matches!(effects.as_slice(), [Effect::Quit]), "{effects:?}");
        assert!(
            app.screen.keys.is_empty(),
            "the screen never sees the quit key"
        );
    }

    #[test]
    fn q_quits_outside_a_text_field() {
        let mut app = app(false);
        let effects = update(&mut app, Msg::Key(key(KeyCode::Char('q'))));
        assert!(app.quit);
        assert!(matches!(effects.as_slice(), [Effect::Quit]), "{effects:?}");
    }

    #[test]
    fn q_is_typed_inside_a_text_field() {
        let mut app = app(true);
        let effects = update(&mut app, Msg::Key(key(KeyCode::Char('q'))));
        assert!(!app.quit);
        assert!(effects.is_empty());
        assert_eq!(app.screen.keys, vec![key(KeyCode::Char('q'))]);
    }

    #[test]
    fn other_messages_reach_the_screen() {
        let mut app = app(false);
        update(&mut app, Msg::Key(key(KeyCode::Char('x'))));
        assert!(update(&mut app, Msg::Tick).is_empty());
        assert!(update(&mut app, Msg::Resize(100, 30)).is_empty());
        assert_eq!(app.screen.keys.len(), 1);
        assert!(!app.quit);
    }

    #[test]
    fn a_rebound_quit_key_quits_and_q_goes_to_the_screen() {
        let text = "[keys]\n\"global.quit\" = \"x\"\n";
        let path = std::path::Path::new("/c/config.toml");
        let config = rail_core::config::parse(text, path).unwrap();
        let keymap = Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap();
        let mut app = app(false).with_keymap(Arc::new(keymap));
        assert!(update(&mut app, Msg::Key(key(KeyCode::Char('q')))).is_empty());
        assert!(!app.quit);
        let effects = update(&mut app, Msg::Key(key(KeyCode::Char('x'))));
        assert!(matches!(effects.as_slice(), [Effect::Quit]), "{effects:?}");
    }

    #[test]
    fn a_screen_that_claims_a_key_keeps_it() {
        let mut app = App::new(
            Hello {
                claims: true,
                ..Hello::default()
            },
            Box::new(RecordingClipboard::default()),
        );
        update(&mut app, Msg::Key(key(KeyCode::Char('q'))));
        assert!(!app.quit);
        assert_eq!(app.screen.keys, vec![key(KeyCode::Char('q'))]);
        update(&mut app, Msg::Key(ctrl('c')));
        assert!(app.quit, "Ctrl-C still quits");
    }

    #[test]
    fn view_draws_the_active_screen_at_both_sizes() {
        let app = app(false);
        for (width, height) in SIZES {
            let lines = render_lines(width, height, |frame| view(&app, frame));
            assert!(lines[0].starts_with("hello from rail"), "{lines:?}");
        }
    }
}
