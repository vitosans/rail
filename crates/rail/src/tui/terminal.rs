//! Raw mode and the alternate screen. `enter` hands back a terminal whose guard restores the
//! user's terminal on every exit path, and the panic hook restores it before a panic prints; each session is restored once, by
//! whichever gets there first.

use std::io::{self, Stdout, Write};
use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::classic_kitty::{Layer, Placement};
use super::handover::Handover;

/// The terminal the shell draws on, plus the guard that puts the user's terminal back.
pub struct Tui {
    /// Declared first so it drops before the guard restores the terminal.
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub(super) handover: Handover,
    _guard: TerminalGuard<Stdout>,
}

impl Drop for Tui {
    fn drop(&mut self) {
        hook_state().set_active(None);
    }
}

/// Give the terminal back to the user (raw mode off, alternate screen left, cursor shown) so a
/// child process can use it. The guard's once-flag is untouched: a later drop of the `Tui` still
/// restores exactly once.
pub fn suspend() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)
}

/// Take the terminal back after `suspend`. The caller clears ratatui's buffers so the next
/// frame repaints in full.
pub fn resume() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)
}

/// Enter raw mode and the alternate screen. The guard exists before the alternate screen
/// is entered, so every later failure still restores the terminal.
pub fn enter() -> io::Result<Tui> {
    install_panic_hook();
    let restored = Restored::default();
    activate_after(hook_state(), restored.clone(), move || {
        enable_raw_mode()?;
        let guard = TerminalGuard::new(io::stdout(), restored.clone());
        execute!(io::stdout(), EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(Tui {
            terminal,
            handover: Handover::for_terminal(restored, io::stdout()),
            _guard: guard,
        })
    })
}

/// Run `setup`, and only if it succeeds record this thread as the one that owns the terminal,
/// with the session's restore flag. A failed setup leaves no `Tui` to clear the record, so it
/// must not be written first.
fn activate_after<T>(
    state: &HookState,
    restored: Restored,
    setup: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let value = setup()?;
    state.set_active(Some(Active {
        thread: thread::current().id(),
        restored,
    }));
    Ok(value)
}

/// One terminal session's restore. Whichever of the panic hook and the guard's drop gets
/// there first restores the terminal; the other writes nothing.
#[derive(Clone, Default)]
pub struct Restored {
    claimed: Arc<AtomicBool>,
    images: Arc<Mutex<Layer>>,
}

impl Restored {
    /// True for the first caller only.
    fn claim(&self) -> bool {
        !self.claimed.swap(true, Ordering::SeqCst)
    }

    pub(super) fn draw_classic(
        &self,
        out: &mut impl Write,
        placements: &[Placement],
    ) -> io::Result<()> {
        let mut layer = self
            .images
            .lock()
            .map_err(|_| io::Error::other("image layer poisoned"))?;
        layer.draw(out, placements)
    }

    /// The screen was cleared under the images: place them all again on the next draw.
    pub(super) fn invalidate_classic(&self) {
        // A poisoned layer fails the next draw anyway.
        if let Ok(mut layer) = self.images.lock() {
            layer.invalidate();
        }
    }
}

/// Restores the terminal when dropped (raw mode off, alternate screen left, cursor shown),
/// unless its session was already restored.
pub struct TerminalGuard<W: Write> {
    out: W,
    restored: Restored,
}

impl<W: Write> TerminalGuard<W> {
    /// Guard `out` for the session `restored` belongs to; nothing is written until the guard
    /// is dropped.
    pub fn new(out: W, restored: Restored) -> Self {
        Self { out, restored }
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        if self.restored.claim() {
            restore(&mut self.out, &self.restored);
        }
    }
}

/// Best effort by design: this runs while dropping or panicking, where an error has nowhere
/// to go, and each step is attempted even if an earlier one failed. Disabling raw mode is a
/// no-op when this process never enabled it.
fn restore(out: &mut impl Write, restored: &Restored) {
    // A panic hook can run while the renderer itself holds the lock.
    if let Ok(mut images) = restored.images.try_lock() {
        let _ = images.draw(out, &[]);
    }
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, Show);
}

/// The session the panic hook looks after: the thread that owns the terminal and its flag.
#[derive(Clone)]
struct Active {
    thread: ThreadId,
    restored: Restored,
}

/// What the panic hook reads: the active session, and how to restore the terminal.
struct HookState {
    active: Mutex<Option<Active>>,
    restore: Box<dyn Fn(&Restored) + Send + Sync>,
}

impl HookState {
    fn set_active(&self, active: Option<Active>) {
        if let Ok(mut slot) = self.active.lock() {
            *slot = active;
        }
    }

    /// Where a panic on this thread happens relative to the TUI.
    fn site(&self) -> PanicSite {
        let owner = self
            .active
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|active| active.thread));
        match owner {
            None => PanicSite::NoTui,
            Some(id) if id == thread::current().id() => PanicSite::TuiThread,
            Some(_) => PanicSite::OtherThread,
        }
    }

    /// Restore the active session's terminal unless its guard (or an earlier call) did.
    fn restore_once(&self) {
        let restored = self
            .active
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|active| active.restored.clone()));
        if let Some(restored) = restored.filter(|restored| restored.claim()) {
            (self.restore)(&restored);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PanicSite {
    /// The thread that entered the TUI: restore the terminal, then print.
    TuiThread,
    /// No TUI is active: the terminal is not ours, so just print.
    NoTui,
    /// A background job while the TUI is up: leave the terminal alone and log.
    OtherThread,
}

type Hook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync>;

/// Restore the terminal and defer to `previous` (which prints) for a panic on the TUI thread;
/// with no TUI active, only defer. A panic on any other thread while the TUI is up is a
/// background job: leave the terminal alone and log one line with the message's length,
/// because the task supervisor reports the message to the screen.
fn panic_hook(state: Arc<HookState>, previous: Hook) -> Hook {
    Box::new(move |info| match state.site() {
        PanicSite::TuiThread => {
            state.restore_once();
            previous(info);
        }
        PanicSite::NoTui => previous(info),
        PanicSite::OtherThread => {
            let location = info.location().map(ToString::to_string).unwrap_or_default();
            tracing::error!(
                location,
                message_bytes = payload_bytes(info),
                "background task panicked"
            );
        }
    })
}

/// The panic message's length when the payload is a string (0 otherwise). The text itself can
/// quote mail or a token, so it never reaches the log; the screen that ran the job shows it.
fn payload_bytes(info: &PanicHookInfo<'_>) -> usize {
    let payload = info.payload();
    payload
        .downcast_ref::<&str>()
        .map(|s| s.len())
        .or_else(|| payload.downcast_ref::<String>().map(String::len))
        .unwrap_or(0)
}

fn hook_state() -> &'static Arc<HookState> {
    static STATE: OnceLock<Arc<HookState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Arc::new(HookState {
            active: Mutex::new(None),
            restore: Box::new(|restored| restore(&mut io::stdout(), restored)),
        })
    })
}

/// Chain a hook that restores the terminal before the existing hook (color-eyre's) prints
/// the panic, for panics on the TUI thread only. Installed once per process.
pub fn install_panic_hook() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(panic_hook(hook_state().clone(), previous));
    });
}

/// Behind the hidden `--panic-test` flag: enter the alternate screen and panic, so a binary
/// test can check that the restore sequence reaches the terminal before the panic message.
/// Raw mode is skipped because that test runs without a terminal.
pub fn panic_test() -> ! {
    install_panic_hook();
    hook_state().set_active(Some(Active {
        thread: thread::current().id(),
        restored: Restored::default(),
    }));
    let _ = execute!(io::stdout(), EnterAlternateScreen);
    panic!("rail panic test: panicking inside the alternate screen");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    /// The panic hook swaps the process-global hook; tests that do so run one at a time.
    static HOOK_TESTS: Mutex<()> = Mutex::new(());

    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("buffer lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn written(buf: &SharedBuf) -> String {
        String::from_utf8(buf.0.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn guard_leaves_the_alternate_screen_and_shows_the_cursor_on_drop() {
        let buf = SharedBuf::default();
        drop(TerminalGuard::new(buf.clone(), Restored::default()));
        let out = written(&buf);
        assert!(out.contains("\x1b[?1049l"), "{out:?}");
        assert!(out.contains("\x1b[?25h"), "{out:?}");
    }

    /// Install a hook over test-owned state, panic once, and return (restored, printed). The
    /// hook is process-global, so this swaps it out and back under a lock.
    fn hook_flags(tui: Option<ThreadId>, panic_on_worker: bool) -> (bool, bool) {
        let _serial = HOOK_TESTS.lock().unwrap();

        let restored = Arc::new(AtomicBool::new(false));
        let printed = Arc::new(AtomicBool::new(false));
        let state = Arc::new(HookState {
            active: Mutex::new(tui.map(|thread| Active {
                thread,
                restored: Restored::default(),
            })),
            restore: {
                let restored = restored.clone();
                Box::new(move |_| restored.store(true, Ordering::SeqCst))
            },
        });
        let previous = {
            let printed = printed.clone();
            Box::new(move |_: &PanicHookInfo<'_>| printed.store(true, Ordering::SeqCst))
        };
        let original = std::panic::take_hook();
        std::panic::set_hook(panic_hook(state, previous));
        let outcome = if panic_on_worker {
            thread::spawn(|| panic!("job boom")).join().map(|_| ())
        } else {
            std::panic::catch_unwind(|| panic!("tui boom"))
        };
        std::panic::set_hook(original);
        assert!(outcome.is_err());
        (
            restored.load(Ordering::SeqCst),
            printed.load(Ordering::SeqCst),
        )
    }

    #[test]
    fn hook_leaves_the_terminal_alone_for_panics_off_the_tui_thread() {
        let flags = hook_flags(Some(thread::current().id()), true);
        assert_eq!(flags, (false, false), "a job panic touched the terminal");
    }

    #[test]
    fn hook_restores_then_prints_for_panics_on_the_tui_thread() {
        let flags = hook_flags(Some(thread::current().id()), false);
        assert_eq!(flags, (true, true));
    }

    #[test]
    fn hook_only_prints_when_no_tui_is_active() {
        assert_eq!(
            hook_flags(None, false),
            (false, true),
            "wrote restore bytes"
        );
        assert_eq!(hook_flags(None, true), (false, true), "wrote restore bytes");
    }

    fn fresh_state() -> HookState {
        HookState {
            active: Mutex::new(None),
            restore: Box::new(|_| {}),
        }
    }

    #[test]
    fn a_failed_enter_leaves_the_tui_thread_unset() {
        let state = fresh_state();
        let result = activate_after(&state, Restored::default(), || -> io::Result<()> {
            Err(io::Error::other("no tty"))
        });
        assert!(result.is_err());
        assert_eq!(state.site(), PanicSite::NoTui);
    }

    #[test]
    fn a_successful_enter_records_the_tui_thread() {
        let state = fresh_state();
        activate_after(&state, Restored::default(), || Ok(())).unwrap();
        assert_eq!(state.site(), PanicSite::TuiThread);
    }

    #[test]
    fn guard_writes_nothing_until_dropped() {
        let buf = SharedBuf::default();
        let guard = TerminalGuard::new(buf.clone(), Restored::default());
        assert!(written(&buf).is_empty());
        drop(guard);
        assert!(!written(&buf).is_empty());
    }

    #[test]
    fn the_hook_and_the_guard_restore_one_session_once() {
        let restores = Arc::new(AtomicUsize::new(0));
        let restored = Restored::default();
        let state = HookState {
            active: Mutex::new(Some(Active {
                thread: thread::current().id(),
                restored: restored.clone(),
            })),
            restore: {
                let restores = restores.clone();
                Box::new(move |_| {
                    restores.fetch_add(1, Ordering::SeqCst);
                })
            },
        };
        let buf = SharedBuf::default();
        let guard = TerminalGuard::new(buf.clone(), restored);
        state.restore_once();
        state.restore_once();
        drop(guard);
        assert_eq!(restores.load(Ordering::SeqCst), 1);
        assert!(written(&buf).is_empty(), "the guard restored a second time");
    }

    #[test]
    fn each_session_restores_once() {
        let buf = SharedBuf::default();
        drop(TerminalGuard::new(buf.clone(), Restored::default()));
        drop(TerminalGuard::new(buf.clone(), Restored::default()));
        assert_eq!(written(&buf).matches("\x1b[?1049l").count(), 2);
    }

    #[test]
    fn a_background_panic_logs_its_length_not_its_text() {
        let _serial = HOOK_TESTS.lock().unwrap();
        let state = Arc::new(HookState {
            active: Mutex::new(Some(Active {
                thread: thread::current().id(),
                restored: Restored::default(),
            })),
            restore: Box::new(|_| {}),
        });
        let original = std::panic::take_hook();
        std::panic::set_hook(panic_hook(state, Box::new(|_: &PanicHookInfo<'_>| {})));
        let worker = thread::spawn(|| {
            crate::tui::test_util::logged(|| {
                std::panic::catch_unwind(|| panic!("job secret text")).is_err()
            })
        });
        let joined = worker.join();
        std::panic::set_hook(original);
        let (panicked, log) = joined.unwrap();
        assert!(panicked);
        assert!(log.contains("background task panicked"), "{log:?}");
        assert!(
            log.contains(&format!("message_bytes={}", "job secret text".len())),
            "{log:?}"
        );
        assert!(!log.contains("job secret text"), "{log:?}");
    }
}
