//! Handing the terminal to a child process (the editor) and taking it back: the event stream
//! is dropped before and made again after, so its reader thread cannot race the child for the
//! tty, and Ctrl-C meant for the child cannot kill Rail.

use std::io::{self, Write};
use std::path::Path;

use crossterm::event::Event as TermEvent;
use futures_util::{Stream, StreamExt};

use super::classic_kitty::Placement;
use super::editor;
use super::terminal;

/// How the loop leaves the terminal to a child process and takes it back.
pub trait Suspend {
    /// Update images after the text frame is flushed. Memory backends have no image layer.
    fn draw_classic(&mut self, _placements: &[Placement]) -> io::Result<()> {
        Ok(())
    }

    /// The screen was cleared: the images it showed must be placed again.
    fn invalidate_classic(&mut self) {}

    /// Give the terminal to the user.
    fn suspend(&mut self) -> io::Result<()>;
    /// Take it back.
    fn resume(&mut self) -> io::Result<()>;
    /// True once after each `resume`: the loop then repaints the whole screen.
    fn take_returned(&mut self) -> bool;
}

/// How `Handover` gives the terminal away or takes it back.
type TerminalStep = fn() -> io::Result<()>;

/// The real terminal's hand-over; the guard in `terminal::enter` still restores it once.
pub struct Handover {
    restored: terminal::Restored,
    picture_output: Box<dyn Write + Send>,
    returned: bool,
    give: TerminalStep,
    take_back: TerminalStep,
}

impl Handover {
    pub(super) fn for_terminal(
        restored: terminal::Restored,
        out: impl Write + Send + 'static,
    ) -> Self {
        Self {
            restored,
            picture_output: Box::new(out),
            returned: false,
            give: terminal::suspend,
            take_back: terminal::resume,
        }
    }
}

#[cfg(test)]
impl Handover {
    /// A hand-over through `give` and `take_back` instead of the real terminal.
    pub(crate) fn with(give: TerminalStep, take_back: TerminalStep) -> Self {
        Self::for_terminal(terminal::Restored::default(), io::sink()).with_steps(give, take_back)
    }

    pub(super) fn with_steps(mut self, give: TerminalStep, take_back: TerminalStep) -> Self {
        self.give = give;
        self.take_back = take_back;
        self
    }
}

impl Suspend for Handover {
    fn draw_classic(&mut self, placements: &[Placement]) -> io::Result<()> {
        self.restored
            .draw_classic(&mut self.picture_output, placements)
    }

    fn invalidate_classic(&mut self) {
        self.restored.invalidate_classic();
    }

    fn suspend(&mut self) -> io::Result<()> {
        self.draw_classic(&[])?;
        (self.give)()
    }

    fn resume(&mut self) -> io::Result<()> {
        // The editor drew on the screen: repaint it whole even if taking it back fails.
        self.returned = true;
        (self.take_back)()
    }

    fn take_returned(&mut self) -> bool {
        std::mem::take(&mut self.returned)
    }
}

/// No terminal to hand over (tests with a `TestBackend` that never run an editor).
impl Suspend for () {
    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn take_returned(&mut self) -> bool {
        false
    }
}

/// The terminal events, from a stream the loop can drop and make again: while the editor runs
/// no reader of the tty may exist, or it would take bytes meant for the editor.
pub struct Input<E, F> {
    stream: Option<E>,
    make: F,
}

impl<E, F> Input<E, F>
where
    E: Stream<Item = io::Result<TermEvent>> + Unpin,
    F: FnMut() -> E,
{
    /// An input reading from the stream `make` builds now (and builds again after an editor).
    pub fn new(mut make: F) -> Self {
        Self {
            stream: Some(make()),
            make,
        }
    }

    /// The next terminal event; `None` when the stream ended.
    pub async fn next(&mut self) -> Option<io::Result<TermEvent>> {
        self.stream.as_mut()?.next().await
    }
}

/// What `apply` needs to run an editor.
pub trait Hand {
    /// Run `command` on `path` with the terminal handed over; its exit code or why not.
    fn run_editor(&mut self, command: &[String], path: &Path) -> Result<i32, String>;
}

/// No editor in tests that never run one.
impl Hand for () {
    fn run_editor(&mut self, _: &[String], _: &Path) -> Result<i32, String> {
        Err("no editor here".to_string())
    }
}

/// The loop's hands: the terminal's hand-over and the event input.
pub struct Hands<'a, X, E, F> {
    pub suspend: &'a mut X,
    pub input: &'a mut Input<E, F>,
}

impl<X, E, F> Hand for Hands<'_, X, E, F>
where
    X: Suspend,
    E: Stream<Item = io::Result<TermEvent>> + Unpin,
    F: FnMut() -> E,
{
    fn run_editor(&mut self, command: &[String], path: &Path) -> Result<i32, String> {
        // Nothing reads the tty but the editor while it runs.
        self.input.stream = None;
        let result = run_editor_blocking(self.suspend, command, path);
        self.input.stream = Some((self.input.make)());
        result
    }
}

/// Hand the terminal to the editor, wait for it, take the terminal back. A failed hand-over
/// still tries to take the terminal back, and says so. Once the editor ran its exit code is
/// the answer: a failure to take the terminal back is logged, and the next frame repaints the
/// whole screen (`resume` marks the return before it tries), so what the editor wrote is still
/// read.
pub fn run_editor_blocking(
    suspend: &mut impl Suspend,
    command: &[String],
    path: &Path,
) -> Result<i32, String> {
    let _quiet = interrupts::Quiet::new();
    if let Err(left) = suspend.suspend() {
        let back = suspend.resume();
        let mut text = format!("cannot leave the terminal for the editor: {left}");
        if let Err(error) = back {
            text.push_str(&format!("; and cannot take it back: {error}"));
        }
        return Err(text);
    }
    let ran = block_here(|| editor::run(command, path));
    if let Err(error) = suspend.resume() {
        tracing::warn!(error = %error, "could not take the terminal back from the editor");
    }
    ran
}

/// Run blocking `work` without starving the runtime's other threads. A current-thread runtime
/// (the tests') has no other threads to keep alive, so the work simply runs.
fn block_here<T>(work: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current().map(|handle| handle.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

/// While an editor runs, SIGINT and SIGQUIT (Ctrl-C and Ctrl-\ from an editor that leaves the
/// tty's signals on reach the whole foreground group) are swallowed by Rail; the editor handles
/// its own. Afterwards they take their default action again.
#[cfg(unix)]
mod interrupts {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    struct State {
        /// While true, the signals take their default action.
        default_action: Arc<AtomicBool>,
        quiet: usize,
    }

    static STATE: Mutex<Option<State>> = Mutex::new(None);

    pub struct Quiet;

    impl Quiet {
        pub fn new() -> Self {
            let mut slot = STATE.lock().unwrap_or_else(PoisonError::into_inner);
            let state = slot.get_or_insert_with(|| State {
                default_action: armed(),
                quiet: 0,
            });
            state.quiet += 1;
            state.default_action.store(false, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for Quiet {
        fn drop(&mut self) {
            let mut slot = STATE.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(state) = slot.as_mut() {
                state.quiet = state.quiet.saturating_sub(1);
                if state.quiet == 0 {
                    state.default_action.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    /// The flag both signals consult. Registered once; true means "default action".
    fn armed() -> Arc<AtomicBool> {
        use signal_hook::consts::{SIGINT, SIGQUIT};
        let flag = Arc::new(AtomicBool::new(true));
        for signal in [SIGINT, SIGQUIT] {
            if let Err(e) = signal_hook::flag::register_conditional_default(signal, flag.clone()) {
                tracing::warn!(error = %e, "could not shield Rail from the editor's interrupts");
            }
        }
        flag
    }
}

#[cfg(not(unix))]
mod interrupts {
    pub struct Quiet;

    impl Quiet {
        pub fn new() -> Self {
            Self
        }
    }
}
