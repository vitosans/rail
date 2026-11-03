//! The composer's one-line prompts, drawn on the bottom row like the filter line: a header
//! prompt (`To`, `Cc`, `Bcc` with address suggestions, `Subject`) and the attachment path
//! prompt with filesystem completion.

use crossterm::event::{KeyCode, KeyEvent};
use rail_core::config::FieldKeys;
use rail_core::store::addresses::AddressEntry;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::file::separators;
use super::prefill::display_parts;
use crate::tui::text::{fit, input_row, pad, width};
use crate::tui::theme::Theme;
use crate::tui::widgets::text_input::TextInput;

/// Matches shown above a prompt.
const SHOWN_MATCHES: usize = 8;
/// The most directory entries a path completion lists.
const MAX_PATH_MATCHES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOutcome {
    Stay,
    Cancel,
    Done(String),
    /// The item being typed changed: ask the store for addresses starting with it.
    NeedSuggestions(String),
}

/// What a prompt draws: its label and input, a note after the input, and the matches.
struct Drawn<'a> {
    prefix: String,
    input: &'a TextInput,
    note: Option<(&'a str, Style)>,
    rows: Vec<String>,
    highlight: Option<usize>,
}

/// The first of `room` rows that keeps `highlight` in view.
fn window_start(len: usize, highlight: Option<usize>, room: usize) -> usize {
    let at = highlight.unwrap_or(0);
    (at + 1).saturating_sub(room).min(len.saturating_sub(1))
}

/// Draw `drawn` in the bottom row of `area`, its matches on the rows above.
fn draw(frame: &mut Frame, area: Rect, theme: &Theme, drawn: &Drawn<'_>) {
    let Some(y) = (area.y + area.height).checked_sub(1) else {
        return;
    };
    draw_matches(frame, area, y, theme, drawn);
    let row = Rect::new(area.x, y, area.width, 1);
    let cols = usize::from(area.width);
    let (value, cursor) = (drawn.input.value(), drawn.input.cursor());
    let (typed, column) = input_row(&drawn.prefix, value, cursor, cols);
    let mut spans = vec![Span::styled(typed.clone(), theme.accent)];
    if let Some((note, style)) = drawn.note {
        let room = cols.saturating_sub(width(&typed));
        spans.push(Span::styled(fit(&format!("   {note}"), room), style));
    }
    frame.render_widget(Clear, row);
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
    if cols > 0 {
        let x = row
            .x
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
        frame.set_cursor_position(Position::new(x, row.y));
    }
}

/// Up to `SHOWN_MATCHES` matches on the rows just above row `y`, the highlighted one in view.
fn draw_matches(frame: &mut Frame, area: Rect, y: u16, theme: &Theme, drawn: &Drawn<'_>) {
    let cols = usize::from(area.width);
    let room = SHOWN_MATCHES.min(usize::from(y - area.y));
    let start = window_start(drawn.rows.len(), drawn.highlight, room);
    let shown: Vec<(usize, &String)> = drawn
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .take(room)
        .collect();
    let top = y - u16::try_from(shown.len()).unwrap_or(0);
    for (offset, (index, text)) in (0u16..).zip(shown) {
        let style = if Some(index) == drawn.highlight {
            theme.selected
        } else {
            theme.text
        };
        let row = Rect::new(area.x, top + offset, area.width, 1);
        frame.render_widget(Clear, row);
        let line = Span::styled(pad(&format!("  {text}"), cols), style);
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// The next index after `at` by `step` (wrapping) in `len` items.
fn cycled(at: Option<usize>, step: isize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = match at {
        None if step < 0 => len - 1,
        None => 0,
        Some(i) => (i as isize + step).rem_euclid(len as isize) as usize,
    };
    Some(next)
}

/// One header's prompt. For `To`, `Cc` and `Bcc` (`completes`), each item typed asks for
/// suggestions; they arrive with none highlighted, Tab and the arrows pick one, and Enter or a
/// comma takes the highlighted one (with none, the typed text and the comma stay as typed).
pub struct HeaderPrompt {
    label: &'static str,
    input: TextInput,
    completes: bool,
    matches: Vec<AddressEntry>,
    highlight: Option<usize>,
    error: Option<String>,
}

impl HeaderPrompt {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    pub fn new(label: &'static str, value: &str, completes: bool) -> Self {
        let mut input = TextInput::default();
        input.set_value(value);
        Self {
            label,
            input,
            completes,
            matches: Vec::new(),
            highlight: None,
            error: None,
        }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    #[cfg(test)]
    pub fn value(&self) -> &str {
        self.input.value()
    }

    /// The highlighted suggestion as it would be written.
    pub fn highlighted(&self) -> Option<String> {
        let entry = self.matches.get(self.highlight?)?;
        Some(display_parts(entry.name.as_deref(), &entry.address))
    }

    /// Show why the value cannot be taken; the prompt stays open.
    pub fn refuse(&mut self, why: String) {
        self.error = Some(why);
    }

    /// The store's answer for the item being typed, shown with none highlighted: Enter must
    /// not replace a fully typed address with a longer known one.
    pub fn suggestions(&mut self, entries: Vec<AddressEntry>) {
        self.highlight = None;
        self.matches = entries;
    }

    /// Where the item being typed starts: after the last comma outside quotes.
    fn item_start(&self) -> usize {
        let (commas, _) = separators(self.input.value());
        commas.last().map_or(0, |i| i + 1)
    }

    /// The item being typed: the text after the last comma outside quotes, trimmed.
    pub fn current_item(&self) -> &str {
        self.input.value()[self.item_start()..].trim()
    }

    /// Put the highlighted match in place of the item being typed, then `after`.
    fn take_highlighted(&mut self, after: &str) -> bool {
        let Some(chosen) = self.highlighted() else {
            return false;
        };
        let head = &self.input.value()[..self.item_start()];
        let joined = if head.is_empty() {
            format!("{chosen}{after}")
        } else {
            format!("{head} {chosen}{after}")
        };
        self.input.set_value(&joined);
        self.matches.clear();
        self.highlight = None;
        true
    }

    pub fn key(&mut self, key: KeyEvent) -> PromptOutcome {
        match key.code {
            KeyCode::Esc if !self.input.takes_esc() => PromptOutcome::Cancel,
            KeyCode::Enter => {
                self.take_highlighted("");
                PromptOutcome::Done(self.input.value().to_string())
            }
            KeyCode::Tab | KeyCode::Down => {
                self.highlight = cycled(self.highlight, 1, self.matches.len());
                PromptOutcome::Stay
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.highlight = cycled(self.highlight, -1, self.matches.len());
                PromptOutcome::Stay
            }
            KeyCode::Char(',') if self.take_highlighted(", ") => PromptOutcome::Stay,
            _ => self.typed(key),
        }
    }

    fn typed(&mut self, key: KeyEvent) -> PromptOutcome {
        if !self.input.handle_key(key) {
            return PromptOutcome::Stay;
        }
        self.error = None;
        self.matches.clear();
        self.highlight = None;
        let item = self.current_item();
        if self.completes && !item.is_empty() {
            PromptOutcome::NeedSuggestions(item.to_string())
        } else {
            PromptOutcome::Stay
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let rows = self
            .matches
            .iter()
            .map(|e| display_parts(e.name.as_deref(), &e.address))
            .collect();
        let drawn = Drawn {
            prefix: format!("{}: ", self.label),
            input: &self.input,
            note: self.error.as_deref().map(|e| (e, theme.error)),
            rows,
            highlight: self.highlight,
        };
        draw(frame, area, theme, &drawn);
    }
}

/// `~` and `~/…` with `$HOME` in front; anything else as typed.
fn expand(path: &str) -> String {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{home}{rest}")
        }
        _ => path.to_string(),
    }
}

/// The names in `dir` starting with `prefix` (hidden ones only when `prefix` starts with
/// `.`), directories with a trailing `/`, at most `MAX_PATH_MATCHES`, sorted.
fn list_dir(dir: &str, prefix: &str) -> std::io::Result<Vec<String>> {
    let dir = if dir.is_empty() { "." } else { dir };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
            continue;
        }
        let marked = if entry.path().is_dir() {
            format!("{name}/")
        } else {
            name
        };
        names.push(marked);
        if names.len() >= MAX_PATH_MATCHES {
            break;
        }
    }
    names.sort();
    Ok(names)
}

/// The longest start every name shares.
fn common_start(names: &[String]) -> &str {
    let Some(first) = names.first() else {
        return "";
    };
    let mut end = first.len();
    for name in &names[1..] {
        end = first
            .char_indices()
            .zip(name.chars())
            .find(|((_, a), b)| a != b)
            .map_or(end.min(name.len()), |((i, _), _)| i.min(end));
    }
    &first[..end]
}

/// The attachment path prompt: Tab lists the directory of what is typed (`~` expanded), a
/// second Tab fills the matches in turn; Enter gives the absolute path.
pub struct PathPrompt {
    input: TextInput,
    /// The typed directory part (up to the last `/`) the matches belong to.
    base: String,
    matches: Vec<String>,
    highlight: Option<usize>,
    note: Option<String>,
}

impl PathPrompt {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    pub fn new(value: &str) -> Self {
        let mut input = TextInput::default();
        input.set_value(value);
        Self {
            input,
            base: String::new(),
            matches: Vec::new(),
            highlight: None,
            note: None,
        }
    }

    #[cfg(test)]
    pub fn value(&self) -> &str {
        self.input.value()
    }

    /// The typed path with `~` expanded.
    pub fn value_expanded(&self) -> String {
        expand(self.input.value().trim())
    }

    #[cfg(test)]
    pub fn matches(&self) -> &[String] {
        &self.matches
    }

    pub fn key(&mut self, key: KeyEvent) -> PromptOutcome {
        match key.code {
            KeyCode::Esc if !self.input.takes_esc() => PromptOutcome::Cancel,
            KeyCode::Enter => {
                let expanded = self.value_expanded();
                if expanded.is_empty() {
                    return PromptOutcome::Cancel;
                }
                let absolute = std::path::absolute(&expanded)
                    .map_or(expanded, |p| p.to_string_lossy().into_owned());
                PromptOutcome::Done(absolute)
            }
            KeyCode::Tab => {
                self.tab();
                PromptOutcome::Stay
            }
            KeyCode::BackTab => {
                self.cycle(-1);
                PromptOutcome::Stay
            }
            _ => {
                if self.input.handle_key(key) {
                    self.matches.clear();
                    self.highlight = None;
                    self.note = None;
                }
                PromptOutcome::Stay
            }
        }
    }

    fn cycle(&mut self, step: isize) {
        self.highlight = cycled(self.highlight, step, self.matches.len());
        if let Some(name) = self.highlight.and_then(|i| self.matches.get(i)) {
            self.input.set_value(&format!("{}{name}", self.base));
        }
    }

    fn tab(&mut self) {
        if !self.matches.is_empty() {
            self.cycle(1);
            return;
        }
        let value = self.input.value().to_string();
        let split = value.rfind('/').map_or(0, |i| i + 1);
        let (base, prefix) = value.split_at(split);
        self.base = base.to_string();
        self.note = None;
        match list_dir(&expand(base), prefix) {
            Err(_) => self.note = Some("cannot list that directory".into()),
            Ok(names) if names.is_empty() => self.note = Some("no match".into()),
            Ok(names) if names.len() == 1 => {
                self.input.set_value(&format!("{base}{}", names[0]));
            }
            Ok(names) => {
                let start = common_start(&names);
                if start.len() > prefix.len() {
                    self.input.set_value(&format!("{base}{start}"));
                }
                self.matches = names;
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let drawn = Drawn {
            prefix: "Attach: ".to_string(),
            input: &self.input,
            note: self.note.as_deref().map(|n| (n, theme.muted)),
            rows: self.matches.clone(),
            highlight: self.highlight,
        };
        draw(frame, area, theme, &drawn);
    }
}
