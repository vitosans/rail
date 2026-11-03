//! The one optional modal over the mail screen: confirmations, the link number prompt, the
//! attachment list, the filter line, the palette, the label and theme pickers, help and `:errors`.
//! Modal keys are fixed (Enter, Esc, `y`/`n`, arrows): they are not keymap actions.

pub mod attachments;
pub mod confirm;
pub mod errors;
pub mod filter;
pub mod help;
pub mod palette;
pub mod picker;
pub mod prompt;
mod scroll;
#[cfg(test)]
mod tests;

use crossterm::event::KeyEvent;
use rail_core::config::FieldKeys;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear};
use url::Url;

pub use attachments::{AttachmentList, AttachmentRow, AttachmentTarget};
pub use confirm::{Confirm, LinkPrompt};
pub use errors::{ErrorList, ErrorRow};
pub use filter::FilterLine;
pub use help::Help;
pub use palette::{Palette, PaletteEntry, Target};
#[cfg(test)]
pub use picker::PICKER_WIDTH;
pub use picker::{AccentAnswer, Picker, PickerItem, PickerKind};
pub use prompt::TextPrompt;

use crate::tui::theme::Theme;

/// What a modal asks the screen to do once it closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    OpenLink(Url),
    /// Show the address of link `number` (of `links`) for confirmation.
    ChooseLink {
        links: Vec<String>,
        number: usize,
    },
    /// Open the message with this id in the browser, once confirmed.
    OpenHtml(i64),
    /// `Esc` in the filter line: drop the filter.
    ClearFilter,
    /// Run what a palette entry stands for.
    Palette(Target),
    /// A thread-action confirmation was answered yes: apply the pending action.
    Trash,
    /// Send this failed outbox row again.
    Retry(rail_core::ops::RowToken),
    /// The label picker's answer: labels to add and to remove.
    Labels {
        add: Vec<i64>,
        remove: Vec<i64>,
    },
    /// The move picker's answer: the label folder to move to.
    MoveTo(i64),
    /// `:theme`'s answer: an index into `theme::scheme::schemes()`, or `None` for Rail's default.
    Theme(Option<usize>),
    /// `:accent`'s answer.
    Accent(AccentAnswer),
    /// `:keymap`'s answer: an index into `KeymapPreset::ALL`.
    Keymap(usize),
    /// "Keep these keys?" was answered yes: save the preset being tried.
    KeepKeymap,
    /// The account picker's answer: write a new message from account `n` (its position).
    Account(usize),
    /// The drafts picker's answer: open the row at this position.
    DraftRow(usize),
    /// The Drafts folder's discard question was answered yes.
    DiscardDraftRow,
    /// "Keep what you wrote?" was answered yes: use the text the editor left.
    KeepText,
    /// "Discard the unsaved changes?" was answered yes: close the composer.
    DiscardUnsaved,
    /// The snooze chooser's answer: snooze its threads until `until` (Unix seconds); `said` is
    /// the text typed for it, which the chooser offers again as "Last used".
    Snooze {
        until: i64,
        said: Option<String>,
    },
    /// The snooze chooser's "Return to inbox now": put its snoozed threads back.
    SnoozeReturn,
    /// Open this attachment with the system opener, asking first once a session. From the
    /// list's `Enter`/`o`, and from the question's own yes.
    OpenAttachment(AttachmentTarget),
    /// Save this attachment alone (the list's `s`, and an `Enter` on a type Rail does not
    /// open).
    SaveAttachment(AttachmentTarget),
    /// Save every attachment of this part's message (the list's `S`).
    SaveAllAttachments(AttachmentTarget),
    /// The name prompt's answer: save the filter in force as a search called `name`.
    SaveSearch {
        name: String,
    },
    /// The name prompt's answer: the saved search `old` — at `index` when the prompt opened —
    /// is renamed to `name`. The old name travels with the index because the list can be
    /// replaced under an open prompt, and an index resolved at `Enter` time would then name a
    /// different search.
    RenameSearch {
        index: usize,
        old: String,
        name: String,
    },
    /// The removal question was answered yes for the saved search `name`, at `index` when the
    /// question was asked. Both are carried for the reason `RenameSearch` gives.
    RemoveSearch {
        index: usize,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalOutcome {
    Stay,
    Close,
    /// The filter text changed; the line stays open.
    Typed(String),
    Run(Command),
    /// The highlight moved in a picker that previews: apply `Command` without keeping it.
    Preview(Command),
}

pub enum Modal {
    Confirm(Confirm),
    LinkNumber(LinkPrompt),
    Attachments(AttachmentList),
    Filter(FilterLine),
    Palette(Palette),
    Picker(Picker),
    Help(Help),
    Errors(ErrorList),
    /// The one-line name prompt behind `S` and `r`.
    Prompt(TextPrompt),
}

impl Modal {
    pub fn key(&mut self, key: KeyEvent) -> ModalOutcome {
        match self {
            Self::Confirm(confirm) => confirm.key(key),
            Self::LinkNumber(prompt) => prompt.key(key),
            Self::Attachments(list) => list.key(key),
            Self::Filter(line) => line.key(key),
            Self::Palette(palette) => palette.key(key),
            Self::Picker(picker) => picker.key(key),
            Self::Help(help) => help.key(key),
            Self::Errors(list) => list.key(key),
            Self::Prompt(prompt) => prompt.key(key),
        }
    }

    /// Hand the modal's text field `general.field_keys`, before a key; the other modals
    /// have no field.
    pub fn use_field_keys(&mut self, keys: FieldKeys) {
        match self {
            Self::Filter(line) => line.use_field_keys(keys),
            Self::Palette(palette) => palette.use_field_keys(keys),
            Self::Picker(picker) => picker.use_field_keys(keys),
            Self::Prompt(prompt) => prompt.use_field_keys(keys),
            Self::Confirm(_)
            | Self::LinkNumber(_)
            | Self::Attachments(_)
            | Self::Help(_)
            | Self::Errors(_) => {}
        }
    }

    /// True while the modal takes typed text (so `q` is typed, not quit).
    pub fn captures_text(&self) -> bool {
        matches!(
            self,
            Self::LinkNumber(_)
                | Self::Filter(_)
                | Self::Palette(_)
                | Self::Picker(_)
                | Self::Prompt(_)
        )
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match self {
            Self::Confirm(confirm) => confirm.render(frame, area, theme),
            Self::LinkNumber(prompt) => prompt.render(frame, area, theme),
            Self::Attachments(list) => list.render(frame, area, theme),
            Self::Filter(line) => line.render(frame, area, theme),
            Self::Palette(palette) => palette.render(frame, area, theme),
            Self::Picker(picker) => picker.render(frame, area, theme),
            Self::Help(help) => help.render(frame, area, theme),
            Self::Errors(list) => list.render(frame, area, theme),
            Self::Prompt(prompt) => prompt.render(frame, area, theme),
        }
    }
}

/// Give `area` the theme's background (after a `Clear`, which resets it); the default theme has none.
pub(crate) fn paint_background(frame: &mut Frame, area: Rect, theme: &Theme) {
    if let Some(bg) = theme.background {
        frame.render_widget(Block::new().style(Style::new().bg(bg)), area);
    }
}

/// A bordered box of `width` x `height` (at most the area) in the middle of `area`, cleared,
/// with `title`; its inside.
pub(crate) fn boxed(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    theme: &Theme,
) -> Rect {
    let [_, row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, cell, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(row);
    frame.render_widget(Clear, cell);
    paint_background(frame, cell, theme);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme.accent)
        .title(format!(" {title} "));
    let inner = block.inner(cell);
    frame.render_widget(block, cell);
    inner
}
