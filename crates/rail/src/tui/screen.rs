//! The `Screen` trait every full-screen view implements: the wizard now, the mail views later.

use crossterm::event::KeyEvent;
use ratatui::Frame;

use super::app::{Effect, Msg};
use super::classic_kitty::Placement;
use super::theme::Theme;

/// One full-screen view driven by the event loop.
pub trait Screen {
    /// React to one message and return the effects the event loop should run.
    fn update(&mut self, msg: Msg) -> Vec<Effect>;

    /// Draw the whole frame.
    fn view(&self, frame: &mut Frame, theme: &Theme);

    /// Classic Kitty placements in the frame just drawn. The terminal emits these
    /// after ratatui has flushed its text cells.
    fn classic_placements(&self) -> Vec<Placement> {
        Vec::new()
    }

    /// The effects to run before the first frame (the mail screen's first reads and its
    /// daemon subscription).
    fn start(&mut self) -> Vec<Effect> {
        Vec::new()
    }

    /// True while focus is in a text field, so `q` is typed instead of quitting.
    fn captures_text(&self) -> bool {
        false
    }

    /// True while the screen is finishing work nothing may interrupt. The quit keys then go
    /// to the screen, which quits (with `Effect::Quit`) once the work is done.
    fn defers_quit(&self) -> bool {
        false
    }

    /// True when the screen binds `key` itself right now (a view key, a pending chord, an open
    /// modal), so the shell's global quit key goes to the screen instead.
    fn claims_key(&self, _key: KeyEvent) -> bool {
        false
    }
}
