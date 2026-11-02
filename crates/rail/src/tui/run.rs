//! The event loop. Terminal events, task results and a spinner tick are polled in one
//! `select!`; each message goes through `update`, each effect is applied, and a frame is
//! drawn after every message.

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{Event as TermEvent, EventStream, KeyEventKind};
use futures_util::Stream;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use super::app::{App, Effect, Job, Msg, Osc52, TaggedResult, TaskResult, TaskSink, update, view};
use super::handover::{Hand, Handover, Hands, Input, Suspend};
use super::keys::Keymap;
use super::screen::Screen;
use super::terminal;
use super::theme::Theme;

/// How often `Msg::Tick` arrives. It exists only so spinners can animate.
pub const TICK: Duration = Duration::from_millis(250);

/// The message a panicking job reports when its payload is not a string.
const PANIC_FALLBACK: &str = "task panicked";

/// Run `screen` full-screen with `keymap` until it quits, restore the terminal, and hand the
/// screen back. SIGHUP and SIGTERM stay graceful (they quit the loop, nothing else) until the
/// caller has dropped whatever cleans up after the screen and calls `finish_run`;
/// a failed run resumes them itself, after the screen is dropped.
pub async fn run_app<S: Screen>(screen: S, keymap: Arc<Keymap>, theme: Theme) -> io::Result<S> {
    let mut tui = terminal::enter()?;
    let mut app = App::new(screen, Box::new(Osc52))
        .with_keymap(keymap)
        .with_theme(theme);
    let result = run_loop(
        &mut tui.terminal,
        &mut app,
        || Box::pin(EventStream::new()),
        &mut Handover::default(),
    )
    .await;
    drop(tui);
    match result {
        Ok(()) => Ok(app.screen),
        Err(e) => {
            drop(app);
            resume_default_signals();
            Err(e)
        }
    }
}

/// Drive `app` until it quits or the event stream `make` builds ends. `make` is called again
/// after each editor, whose stream is dropped first. Tasks still running when this returns
/// are aborted, because `Tasks` aborts on drop.
pub async fn run_loop<B, S, E, F, X>(
    terminal: &mut Terminal<B>,
    app: &mut App<S>,
    make: F,
    suspend: &mut X,
) -> io::Result<()>
where
    B: Backend,
    S: Screen,
    E: Stream<Item = io::Result<TermEvent>> + Unpin,
    F: FnMut() -> E,
    X: Suspend,
{
    let mut input = Input::new(make);
    let (sink, mut results) = mpsc::unbounded_channel();
    let mut tasks = Tasks::new(sink);
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut signals = QuitSignals::new()?;
    let mut pending: VecDeque<Msg> = VecDeque::new();
    for effect in app.screen.start() {
        pending.extend(apply(
            effect,
            app,
            &mut tasks,
            &mut Hands {
                suspend,
                input: &mut input,
            },
        ));
    }
    for msg in pending {
        dispatch(
            msg,
            app,
            &mut tasks,
            &mut Hands {
                suspend,
                input: &mut input,
            },
        );
    }
    redraw(terminal, app, suspend)?;
    while !app.quit {
        let msg = tokio::select! {
            event = input.next() => match event {
                Some(Ok(event)) => match to_msg(event) {
                    Some(msg) => msg,
                    None => continue,
                },
                Some(Err(e)) => return Err(e),
                None => return Ok(()),
            },
            Some(tagged) = results.recv() => match tasks.current(tagged) {
                Some(result) => Msg::Task(result),
                None => continue,
            },
            _ = tick.tick() => Msg::Tick,
            () = signals.recv() => {
                app.quit = true;
                continue;
            }
        };
        dispatch(
            msg,
            app,
            &mut tasks,
            &mut Hands {
                suspend,
                input: &mut input,
            },
        );
        redraw(terminal, app, suspend)?;
    }
    Ok(())
}

/// SIGHUP (the terminal window closed) and SIGTERM. Either ends the loop as `q` does, so the
/// screen is dropped (and the files `o` wrote removed) and the terminal restored. Tokio never
/// removes its handlers, so the signals' default action is re-armed by
/// `finish_run`, called once the terminal is restored and the screen is dropped;
/// from then on (the wizard's settle, the final report) they end the process as they would
/// any program. Until then a signal is recorded and the cleanup finishes.
#[cfg(unix)]
struct QuitSignals {
    hangup: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl QuitSignals {
    fn new() -> io::Result<Self> {
        Self::listening(default_action_flag()?.as_ref())
    }

    /// Listen, and hold `default_action` false so the signals are not deadly meanwhile.
    fn listening(default_action: &AtomicBool) -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        default_action.store(false, Ordering::SeqCst);
        Ok(Self {
            hangup: signal(SignalKind::hangup())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    async fn recv(&mut self) {
        tokio::select! {
            _ = self.hangup.recv() => {}
            _ = self.terminate.recv() => {}
        }
    }
}

/// The end of a run, once the terminal is restored and the screen dropped: SIGHUP and SIGTERM
/// take their default action again. In debug builds a test hook then waits, so a binary test
/// can signal Rail while it is past its run loop (as during the wizard's settle).
pub async fn finish_run() {
    resume_default_signals();
    if let Some(linger) = crate::test_hooks::linger_after_loop() {
        tokio::time::sleep(linger).await;
    }
}

/// Let SIGHUP and SIGTERM take their default action (terminate) again. Call it after the run
/// loop's terminal is restored and its screen dropped; nothing is listening any more.
#[cfg(unix)]
pub fn resume_default_signals() {
    match default_action_flag() {
        Ok(flag) => flag.store(true, Ordering::SeqCst),
        Err(e) => tracing::warn!(error = %e, "could not restore the signals' default action"),
    }
}

/// The process-wide flag that, while true, makes SIGHUP and SIGTERM take their default action
/// (terminate). Registered once; a run loop holds it false while it listens. One loop runs at
/// a time in the binary; parallel unit-test loops only flip it in a process nobody signals.
#[cfg(unix)]
fn default_action_flag() -> io::Result<Arc<AtomicBool>> {
    use signal_hook::consts::{SIGHUP, SIGTERM};
    static FLAG: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
    let mut slot = FLAG
        .lock()
        .map_err(|_| io::Error::other("the signal flag lock is poisoned"))?;
    if let Some(flag) = slot.as_ref() {
        return Ok(flag.clone());
    }
    let flag = Arc::new(AtomicBool::new(false));
    for signal in [SIGHUP, SIGTERM] {
        signal_hook::flag::register_conditional_default(signal, flag.clone())?;
    }
    *slot = Some(flag.clone());
    Ok(flag)
}

/// No such signals here: nothing to resume.
#[cfg(not(unix))]
pub fn resume_default_signals() {}

/// No such signals here: never fires.
#[cfg(not(unix))]
struct QuitSignals;

#[cfg(not(unix))]
impl QuitSignals {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    async fn recv(&mut self) {
        std::future::pending::<()>().await;
    }
}

fn to_msg(event: TermEvent) -> Option<Msg> {
    match event {
        TermEvent::Key(key) if key.kind != KeyEventKind::Release => Some(Msg::Key(key)),
        TermEvent::Resize(width, height) => Some(Msg::Resize(width, height)),
        _ => None,
    }
}

/// Run `msg` through `update` and apply its effects. An effect that reports back (a copy)
/// yields a follow-up message, which is handled before the next event.
fn dispatch<S: Screen>(msg: Msg, app: &mut App<S>, tasks: &mut Tasks, hand: &mut impl Hand) {
    let mut pending = VecDeque::from([msg]);
    while let Some(msg) = pending.pop_front() {
        for effect in update(app, msg) {
            pending.extend(apply(effect, app, tasks, hand));
        }
    }
}

fn apply<S: Screen>(
    effect: Effect,
    app: &mut App<S>,
    tasks: &mut Tasks,
    hand: &mut impl Hand,
) -> Option<Msg> {
    match effect {
        Effect::Quit => app.quit = true,
        Effect::CancelAll => tasks.cancel_all(),
        Effect::Spawn(job) => tasks.spawn(job),
        Effect::Theme(theme) => app.theme = theme,
        Effect::Keymap(keymap) => app.keymap = keymap,
        Effect::Copy(text) => {
            let result = app.clipboard.copy(&text).map_err(|e| e.to_string());
            if let Err(reason) = &result {
                tracing::warn!(error = %reason, "could not copy to the clipboard");
            }
            return Some(Msg::Task(TaskResult::Copied(result)));
        }
        Effect::RunEditor { command, path } => {
            // Nothing else runs while the editor has the terminal: the loop blocks here.
            let result = hand.run_editor(&command, &path);
            return Some(Msg::Task(TaskResult::EditorDone(result)));
        }
    }
    None
}

/// Draw a frame; after the terminal came back from the editor, repaint all of it. The repaint
/// is `resize` to the current size, not `clear`: `clear` asks the terminal for the cursor
/// position, which cannot be read while the event stream's reader waits for input.
fn redraw<B: Backend, S: Screen>(
    terminal: &mut Terminal<B>,
    app: &App<S>,
    suspend: &mut impl Suspend,
) -> io::Result<()> {
    if suspend.take_returned() {
        let size = terminal
            .size()
            .map_err(|e| io::Error::other(e.to_string()))?;
        terminal
            .resize(Rect::from(size))
            .map_err(|e| io::Error::other(e.to_string()))?;
    }
    draw(terminal, app)
}

fn draw<B: Backend, S: Screen>(terminal: &mut Terminal<B>, app: &App<S>) -> io::Result<()> {
    terminal
        .draw(|frame| view(app, frame))
        .map(|_| ())
        .map_err(|e| io::Error::other(e.to_string()))
}

/// Aborts the wrapped task when dropped, so cancelling a supervisor cancels its job too.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The tasks spawned for effects. Dropping it aborts whatever is still running, so leaving
/// the loop (quit, error, or the screen going away) cancels every effect.
pub struct Tasks {
    tx: mpsc::UnboundedSender<TaggedResult>,
    /// Bumped by every `cancel_all`; results from an older generation are stale.
    generation: u64,
    handles: Vec<JoinHandle<()>>,
}

impl Tasks {
    /// A task set whose jobs report to `tx`.
    pub fn new(tx: mpsc::UnboundedSender<TaggedResult>) -> Self {
        Self {
            tx,
            generation: 0,
            handles: Vec::new(),
        }
    }

    /// The result itself if it came from a job spawned since the last `cancel_all`, else
    /// `None`. A job aborted mid-send, or results already queued when the cancel happened,
    /// are dropped here.
    pub fn current(&self, (generation, result): TaggedResult) -> Option<TaskResult> {
        (generation == self.generation).then_some(result)
    }

    /// Start `job`. If it panics, the panic message goes to the sink as
    /// `TaskResult::JobPanicked`; if it is cancelled, nothing is sent.
    pub fn spawn(&mut self, job: Job) {
        self.handles.retain(|handle| !handle.is_finished());
        let sink = TaskSink::new(self.tx.clone(), self.generation);
        let work = AbortOnDrop(tokio::spawn({
            let sink = sink.clone();
            async move { job(sink).await }
        }));
        self.handles.push(tokio::spawn(supervise(work, sink)));
    }

    /// Abort every running job and discard anything they already sent.
    pub fn cancel_all(&mut self) {
        self.generation += 1;
        for handle in self.handles.drain(..) {
            handle.abort();
        }
    }

    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.handles.iter().filter(|h| !h.is_finished()).count()
    }
}

impl Drop for Tasks {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// Wait for a job and report a panic. Only string payloads are read; anything else gets a
/// fixed message so arbitrary data never reaches the screen.
async fn supervise(mut work: AbortOnDrop, sink: TaskSink) {
    let Err(error) = (&mut work.0).await else {
        return;
    };
    if !error.is_panic() {
        return;
    }
    let payload = error.into_panic();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| PANIC_FALLBACK.to_string());
    let _ = sink.send(TaskResult::JobPanicked(message));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crossterm::event::{KeyCode, KeyEvent};
    use futures_util::stream;
    use ratatui::backend::TestBackend;
    use tokio::sync::oneshot;

    use crate::tui::app::TaskResult;
    use crate::tui::test_util::{Hello, RecordingClipboard, ctrl, key, once};
    use ratatui::style::Color;

    #[test]
    fn a_theme_effect_replaces_the_apps_theme() {
        let mut app = App::new(Hello::default(), Box::new(RecordingClipboard::default()));
        let mut tasks = Tasks::new(mpsc::unbounded_channel().0);
        let theme = Theme {
            background: Some(Color::Rgb(9, 9, 9)),
            ..Theme::default()
        };
        assert!(apply(Effect::Theme(theme), &mut app, &mut tasks, &mut ()).is_none());
        assert_eq!(app.theme, theme);
    }

    #[test]
    fn a_keymap_effect_replaces_the_keys_the_shell_quits_with() {
        let mut app = App::new(Hello::default(), Box::new(RecordingClipboard::default()));
        let mut tasks = Tasks::new(mpsc::unbounded_channel().0);
        let text = "[keys]\n\"global.quit\" = \"Q\"\n";
        let path = std::path::Path::new("/c/config.toml");
        let config = rail_core::config::parse(text, path).unwrap();
        let keymap = Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap();
        let keymap = std::sync::Arc::new(keymap);
        assert!(
            apply(
                Effect::Keymap(keymap.clone()),
                &mut app,
                &mut tasks,
                &mut ()
            )
            .is_none()
        );
        assert_eq!(app.keymap, keymap);
    }

    /// Dropping the listeners (when `run_loop` returns) must not make the signals deadly: the
    /// terminal is not restored yet and the screen not dropped. Only
    /// `resume_default_signals` does that, after both.
    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_the_listeners_leaves_the_default_action_off() {
        let flag = AtomicBool::new(true);
        let signals = QuitSignals::listening(&flag).unwrap();
        assert!(!flag.load(Ordering::SeqCst), "listening arms the default");
        drop(signals);
        assert!(!flag.load(Ordering::SeqCst), "the drop armed the default");
    }

    fn keys(keys: Vec<KeyEvent>) -> impl Stream<Item = io::Result<TermEvent>> + Unpin {
        stream::iter(
            keys.into_iter()
                .map(|k| Ok(TermEvent::Key(k)))
                .collect::<Vec<_>>(),
        )
    }

    struct SetOnDrop(Arc<AtomicBool>);

    impl Drop for SetOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// A job that never finishes on its own and records when it is dropped. The returned
    /// sender keeps it waiting; drop it only after the assertion.
    fn endless_job(dropped: Arc<AtomicBool>) -> (Job, oneshot::Sender<()>) {
        let (keep_open, never) = oneshot::channel::<()>();
        let flag = SetOnDrop(dropped);
        let job: Job = Box::new(move |_sink| {
            Box::pin(async move {
                let _flag = flag;
                let _ = never.await;
            })
        });
        (job, keep_open)
    }

    async fn becomes_true(flag: &AtomicBool) -> bool {
        for _ in 0..1000 {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            tokio::task::yield_now().await;
        }
        false
    }

    fn terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(30, 3)).unwrap()
    }

    #[tokio::test]
    async fn loop_draws_the_screen_and_quits_on_q() {
        let mut terminal = terminal();
        let mut app = App::new(Hello::default(), Box::new(RecordingClipboard::default()));
        let events = keys(vec![key(KeyCode::Char('x')), key(KeyCode::Char('q'))]);
        run_loop(&mut terminal, &mut app, once(events), &mut ())
            .await
            .unwrap();
        assert!(app.quit);
        assert_eq!(app.screen.keys, vec![key(KeyCode::Char('x'))]);
        let row: String = (0..15)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert_eq!(row, "hello from rail");
    }

    #[tokio::test]
    async fn start_effects_run_before_the_first_key() {
        let mut terminal = terminal();
        let screen = Hello {
            quit_at_start: true,
            ..Hello::default()
        };
        let mut app = App::new(screen, Box::new(RecordingClipboard::default()));
        let events = keys(vec![key(KeyCode::Char('x'))]);
        run_loop(&mut terminal, &mut app, once(events), &mut ())
            .await
            .unwrap();
        assert!(app.quit);
        assert!(app.screen.keys.is_empty(), "quit before reading a key");
    }

    #[tokio::test]
    async fn quitting_aborts_tasks_still_in_flight() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (job, _keep_open) = endless_job(dropped.clone());
        let mut job = Some(job);
        let screen = Hello {
            on_key: Box::new(move |k| {
                if k.code == KeyCode::Char('s') {
                    job.take().map(Effect::Spawn).into_iter().collect()
                } else {
                    Vec::new()
                }
            }),
            ..Hello::default()
        };
        let mut terminal = terminal();
        let mut app = App::new(screen, Box::new(RecordingClipboard::default()));
        let events = keys(vec![key(KeyCode::Char('s')), ctrl('c')]);
        run_loop(&mut terminal, &mut app, once(events), &mut ())
            .await
            .unwrap();
        assert!(
            becomes_true(&dropped).await,
            "the in-flight job was not aborted"
        );
    }

    #[tokio::test]
    async fn cancel_all_aborts_running_jobs() {
        let (sink, _results) = mpsc::unbounded_channel();
        let mut tasks = Tasks::new(sink);
        let dropped = Arc::new(AtomicBool::new(false));
        let (job, _keep_open) = endless_job(dropped.clone());
        tasks.spawn(job);
        tokio::task::yield_now().await;
        assert_eq!(tasks.in_flight(), 1);
        tasks.cancel_all();
        assert!(becomes_true(&dropped).await);
        assert_eq!(tasks.in_flight(), 0);
    }

    #[tokio::test]
    async fn cancelled_jobs_report_nothing() {
        let (sink, mut results) = mpsc::unbounded_channel();
        let mut tasks = Tasks::new(sink);
        let dropped = Arc::new(AtomicBool::new(false));
        let (job, _keep_open) = endless_job(dropped.clone());
        tasks.spawn(job);
        tokio::task::yield_now().await;
        tasks.cancel_all();
        assert!(becomes_true(&dropped).await);
        drop(tasks);
        assert!(
            results.recv().await.is_none(),
            "a cancelled job sent a result"
        );
    }

    #[tokio::test]
    async fn results_sent_before_cancel_all_are_dropped() {
        let (tx, mut results) = mpsc::unbounded_channel();
        let mut tasks = Tasks::new(tx);
        let (sent_tx, sent_rx) = oneshot::channel();
        let (_keep_open, never) = oneshot::channel::<()>();
        tasks.spawn(Box::new(move |sink| {
            Box::pin(async move {
                let _ = sink.send(TaskResult::Copied(Ok(())));
                let _ = sent_tx.send(());
                let _ = never.await;
            })
        }));
        sent_rx.await.unwrap();
        tasks.cancel_all();
        tasks.spawn(Box::new(|sink| {
            Box::pin(async move {
                let _ = sink.send(TaskResult::Copied(Err("fresh".into())));
            })
        }));
        let mut delivered = Vec::new();
        for _ in 0..2 {
            let tagged = results.recv().await.expect("both results were queued");
            delivered.extend(tasks.current(tagged));
        }
        match delivered.as_slice() {
            [TaskResult::Copied(Err(reason))] => assert_eq!(reason, "fresh"),
            other => panic!("expected only the post-cancel result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_panicking_job_is_reported_with_its_message() {
        let (sink, mut results) = mpsc::unbounded_channel();
        let mut tasks = Tasks::new(sink);
        tasks.spawn(Box::new(|_sink| Box::pin(async { panic!("boom") })));
        match results.recv().await {
            Some((_, TaskResult::JobPanicked(message))) => assert_eq!(message, "boom"),
            other => panic!("expected JobPanicked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn copy_effects_go_to_the_clipboard_and_report_back() {
        let clipboard = RecordingClipboard::default();
        let screen = Hello {
            on_key: Box::new(|k| {
                if k.code == KeyCode::Char('y') {
                    vec![Effect::Copy("consent url".into())]
                } else {
                    Vec::new()
                }
            }),
            ..Hello::default()
        };
        let mut terminal = terminal();
        let mut app = App::new(screen, Box::new(clipboard.clone()));
        let events = keys(vec![key(KeyCode::Char('y')), key(KeyCode::Char('q'))]);
        run_loop(&mut terminal, &mut app, once(events), &mut ())
            .await
            .unwrap();
        assert_eq!(
            *clipboard.0.lock().unwrap(),
            vec!["consent url".to_string()]
        );
        assert_eq!(app.screen.copied, vec![Ok(())]);
    }
}
