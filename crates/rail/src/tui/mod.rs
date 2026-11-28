//! The terminal shell: raw mode and the alternate screen, the event loop, and the screens
//! that run inside it. Nothing here prints text to stdout or stderr while the alternate
//! screen is up.

pub mod app;
pub(crate) mod classic_kitty;
#[cfg(test)]
mod classic_lifetime_tests;
pub mod clock;
pub mod data;
pub mod editor;
#[cfg(test)]
mod editor_tests;
pub mod effects;
pub mod handover;
#[cfg(test)]
mod handover_tests;
pub mod keys;
pub mod modals;
pub mod open;
pub mod run;
pub mod screen;
pub mod screens;
pub mod snooze;
pub mod terminal;
#[cfg(test)]
pub mod test_util;
pub mod text;
#[cfg(test)]
mod text_tests;
pub mod theme;
pub mod widgets;
