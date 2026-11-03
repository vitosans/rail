//! The `:` palette: every command, account and label, fuzzy-matched with `nucleo-matcher` as
//! you type, each with the key bound to it.

use crossterm::event::{KeyCode, KeyEvent};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use rail_core::config::FieldKeys;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{Command, ModalOutcome, boxed};
use crate::tui::data::ScopeChoice;
use crate::tui::keys::Action;
use crate::tui::text::{escape, input_row, pad, width};
use crate::tui::theme::Theme;
use crate::tui::widgets::text_input::TextInput;

/// Rows of matches shown.
const SHOWN: usize = 12;
/// The palette's width, its border included.
const PALETTE_WIDTH: u16 = 70;

/// What an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Action(Action),
    Errors,
    /// `:theme`: the colour scheme picker.
    Theme,
    /// `:accent`: the accent slot picker.
    Accent,
    /// `:keymap`: the key preset picker.
    Keymap,
    /// `:hints`: turn the key hints off or on.
    Hints,
    /// `:clock`: show or hide the time of day in the header.
    Clock,
    /// `:time`: 24-hour or 12-hour times.
    Time,
    /// `:thread-order`: the reader's older messages oldest or newest first.
    ThreadOrder,
    /// `:dim-read`: dim the dot, subject or all text of read threads.
    DimRead,
    /// `:field-keys`: switch how text fields edit.
    FieldKeys,
    Scope(ScopeChoice),
    /// Write a new message (`c`).
    Compose,
    /// `:drafts`: the drafts picker.
    Drafts,
    /// `:save-search`: name the filter in force and save it.
    SaveSearch,
    /// `rename-search <name>`: rename the saved search this entry showed. The name travels
    /// with the position because the entries are built once, when the palette opens, and a
    /// write's answer can replace the list while it is up — so a position alone would reach
    /// whatever has since moved into it, or nothing.
    RenameSearch {
        index: usize,
        name: String,
    },
    /// `remove-search <name>`: remove the saved search this entry showed, carried for the
    /// reason `RenameSearch` gives.
    RemoveSearch {
        index: usize,
        name: String,
    },
    /// `:search-counts`: show or hide the unread count beside each saved search.
    SearchCounts,
    /// `:rich-html`: draw HTML bodies with their styles or as plain text.
    RichHtml,
    /// `:images`: draw rich bodies' pictures or leave them out.
    Images,
    /// `:remote-images`: fetch remote pictures always, or only when a thread's are loaded.
    RemoteImages,
}

/// One entry: what it shows (raw: label names are outside text), what it is matched by, and
/// the key bound to it (empty when none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub text: String,
    pub search: String,
    pub key: String,
    pub target: Target,
}

struct Candidate<'a> {
    index: usize,
    text: &'a str,
}

impl AsRef<str> for Candidate<'_> {
    fn as_ref(&self) -> &str {
        self.text
    }
}

pub struct Palette {
    entries: Vec<PaletteEntry>,
    input: TextInput,
    matcher: Matcher,
    /// Indices of the matching entries, best first.
    shown: Vec<usize>,
    selected: usize,
}

impl Palette {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    pub fn new(entries: Vec<PaletteEntry>) -> Self {
        let shown = (0..entries.len()).collect();
        Self {
            entries,
            input: TextInput::default(),
            matcher: Matcher::new(Config::DEFAULT),
            shown,
            selected: 0,
        }
    }

    pub fn shown(&self) -> Vec<&PaletteEntry> {
        self.shown
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .collect()
    }

    fn refilter(&mut self) {
        let query = self.input.value();
        self.shown = if query.trim().is_empty() {
            (0..self.entries.len()).collect()
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let candidates = self.entries.iter().enumerate().map(|(index, e)| Candidate {
                index,
                text: &e.search,
            });
            pattern
                .match_list(candidates, &mut self.matcher)
                .into_iter()
                .map(|(candidate, _)| candidate.index)
                .collect()
        };
        self.selected = 0;
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> ModalOutcome {
        match key.code {
            KeyCode::Esc if !self.input.takes_esc() => ModalOutcome::Close,
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.shown.len().saturating_sub(1));
                ModalOutcome::Stay
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                ModalOutcome::Stay
            }
            KeyCode::Enter => self
                .shown
                .get(self.selected)
                .and_then(|&i| self.entries.get(i))
                .map_or(ModalOutcome::Stay, |e| {
                    ModalOutcome::Run(Command::Palette(e.target.clone()))
                }),
            _ => {
                if self.input.handle_key(key) {
                    self.refilter();
                }
                ModalOutcome::Stay
            }
        }
    }

    pub(super) fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = u16::try_from(SHOWN + 4).unwrap_or(u16::MAX);
        let inner = boxed(frame, area, PALETTE_WIDTH, height, "Command palette", theme);
        let cols = usize::from(inner.width);
        let (typed, column) = input_row(":", self.input.value(), self.input.cursor(), cols);
        let mut lines = vec![Line::styled(typed, theme.accent), Line::default()];
        // A window of SHOWN rows that keeps the selection in view.
        let first = (self.selected + 1).saturating_sub(SHOWN);
        for (row, entry) in self.shown().into_iter().enumerate().skip(first).take(SHOWN) {
            let style = if row == self.selected {
                theme.selected
            } else {
                theme.text
            };
            let key = escape(&entry.key);
            let room = cols.saturating_sub(width(&key) + 1);
            lines.push(Line::from(vec![
                Span::styled(pad(&escape(&entry.text), room), style),
                Span::raw(" "),
                Span::styled(key, theme.muted),
            ]));
        }
        if self.shown.is_empty() {
            lines.push(Line::styled("Nothing matches.", theme.muted));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        if cols > 0 && inner.height > 0 {
            let x = inner
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            frame.set_cursor_position(Position::new(x, inner.y));
        }
    }
}
