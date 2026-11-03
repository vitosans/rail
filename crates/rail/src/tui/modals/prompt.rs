//! The one-line name prompt: `S` saves the filter in force under a name, `r` renames a saved
//! search. `Enter` runs `check` and refuses in place, so a clashing or malformed name never
//! reaches the daemon; `Esc` cancels.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent};
use rail_core::config::FieldKeys;
use rail_core::store::filter::MAX_FILTER_CHARS;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::{Command, ModalOutcome, boxed};
use crate::tui::text::{escape, input_row, wrap};
use crate::tui::theme::Theme;
use crate::tui::widgets::text_input::TextInput;

/// What `Enter` runs before the prompt closes: `Err(reason)` refuses in place.
pub type Check = Box<dyn Fn(&str) -> Result<(), String> + Send>;
/// What a valid line becomes. Boxed, not a bare `fn(String) -> Command`, because a rename's
/// command carries the row's index and so has to be closed over.
pub type Make = Box<dyn Fn(String) -> Command + Send>;

/// The prompt box's width, its border included.
pub const PROMPT_WIDTH: u16 = 56;
/// The hint under the line, replaced by a refusal.
const HINT: &str = "Enter save · Esc cancel";

/// A one-line text prompt that validates before it closes.
pub struct TextPrompt {
    title: String,
    label: String,
    input: TextInput,
    /// Runs on `Enter`: `Err(reason)` keeps the box open showing `reason`.
    check: Check,
    /// What a valid name becomes.
    make: Make,
    /// The last refusal, shown in place of the hint row.
    refusal: Option<String>,
}

impl fmt::Debug for TextPrompt {
    /// The title and how much was typed: a name is outside text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextPrompt")
            .field("title", &self.title)
            .field("len", &self.input.value().chars().count())
            .field("refused", &self.refusal.is_some())
            .finish_non_exhaustive()
    }
}

impl TextPrompt {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    pub fn new(title: &str, label: &str, start: &str, check: Check, make: Make) -> Self {
        let mut input = TextInput::default();
        input.set_value(start);
        Self {
            title: title.to_string(),
            label: label.to_string(),
            input,
            check,
            make,
            refusal: None,
        }
    }

    /// Show `reason` in place of the hint row and keep the box open.
    pub fn refuse(&mut self, reason: String) {
        self.refusal = Some(reason);
    }

    /// The refusal on show, if any.
    #[cfg(test)]
    pub(crate) fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Edit the line; `Enter` validates through `check` and only a valid line leaves the
    /// prompt, so FR-4's and FR-5's refusals happen before any call; `Esc` cancels.
    ///
    /// Typing stops at `MAX_FILTER_CHARS`, far above the name cap, so a held key cannot grow
    /// the line without bound while an over-long name still reaches `check` and is refused in
    /// the wording §8 pins.
    pub fn key(&mut self, key: KeyEvent) -> ModalOutcome {
        match key.code {
            KeyCode::Enter => match (self.check)(self.input.value()) {
                Ok(()) => ModalOutcome::Run((self.make)(self.input.value().to_string())),
                Err(reason) => {
                    self.refuse(reason);
                    ModalOutcome::Stay
                }
            },
            KeyCode::Esc => ModalOutcome::Close,
            _ => {
                let before = self.input.clone();
                if !self.input.handle_key(key) {
                    return ModalOutcome::Stay;
                }
                if self.input.value().chars().count() > MAX_FILTER_CHARS {
                    self.input = before;
                    return ModalOutcome::Stay;
                }
                // The refusal was about the text that was sent; this is different text.
                self.refusal = None;
                ModalOutcome::Stay
            }
        }
    }

    /// The box, the line with its cursor, and the hint or the refusal. A refusal is wrapped
    /// and the box grows by its rows, as a confirmation's body does: the longest refusal is
    /// 79 cells against a 54-cell row, so one row would end it in an ellipsis.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let cols = usize::from(area.width.min(PROMPT_WIDTH).saturating_sub(2)).max(1);
        let (hint, style) = match &self.refusal {
            Some(reason) => (wrap(&escape(reason), cols), theme.error),
            None => (vec![HINT.to_string()], theme.muted),
        };
        let height = u16::try_from(hint.len() + 4).unwrap_or(u16::MAX);
        let inner = boxed(frame, area, PROMPT_WIDTH, height, &self.title, theme);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let (typed, column) = input_row(
            &self.label,
            self.input.value(),
            self.input.cursor(),
            usize::from(inner.width),
        );
        let mut lines = vec![Line::styled(typed, theme.text), Line::default()];
        lines.extend(hint.into_iter().map(|row| Line::styled(row, style)));
        frame.render_widget(Paragraph::new(lines), inner);
        let x = inner
            .x
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
        frame.set_cursor_position(Position::new(x, inner.y));
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::tui::modals::ModalOutcome;
    use crate::tui::test_util::{key, render_lines};

    /// A prompt over `start` that refuses anything holding a space, as the saved-search
    /// prompt refuses a malformed name.
    fn prompt(start: &str) -> TextPrompt {
        TextPrompt::new(
            "Save this filter as a search",
            "Name: ",
            start,
            Box::new(|name: &str| {
                if name.contains(' ') {
                    Err("A saved search name must be 1 to 40 letters, digits, '-' or '_'.".into())
                } else {
                    Ok(())
                }
            }),
            Box::new(|name| Command::SaveSearch { name }),
        )
    }

    fn typed(prompt: &mut TextPrompt, text: &str) {
        for c in text.chars() {
            prompt.key(key(KeyCode::Char(c)));
        }
    }

    /// FR-1: the line is edited in place, `Enter` makes the command and `Esc` cancels.
    #[test]
    fn the_prompt_edits_a_line_and_enter_makes_the_command() {
        let mut prompt = prompt("");
        typed(&mut prompt, "needs-reply!");
        assert_eq!(prompt.key(key(KeyCode::Backspace)), ModalOutcome::Stay);
        assert_eq!(
            prompt.key(key(KeyCode::Enter)),
            ModalOutcome::Run(Command::SaveSearch {
                name: "needs-reply".into()
            })
        );
        assert_eq!(prompt.refusal(), None);
        assert_eq!(prompt.key(key(KeyCode::Esc)), ModalOutcome::Close);
    }

    /// FR-4/FR-5: `Enter` runs the check first, so a refused name never leaves the prompt;
    /// the box stays open with the text kept, and the next edit clears the refusal.
    ///
    /// The refusal's drawing is asserted whole, at both mail-screen sizes (§6 slot 2). FR-5's
    /// sentence is 64 cells against the 54-cell inner row, so an unwrapped row cut by `fit`
    /// would still hold its opening words: only the tail, the absence of an ellipsis and the
    /// box's second refusal row tell a wrapped drawing from a cut one.
    #[test]
    fn a_refused_name_keeps_the_prompt_open_and_is_shown() {
        let mut prompt = prompt("bad name");
        assert_eq!(prompt.key(key(KeyCode::Enter)), ModalOutcome::Stay);
        assert_eq!(
            prompt.refusal(),
            Some("A saved search name must be 1 to 40 letters, digits, '-' or '_'.")
        );
        for (width, height) in [(90_u16, 24_u16), (150, 40)] {
            let rows = render_lines(width, height, |frame| {
                prompt.render(frame, frame.area(), &Theme::default());
            });
            let shown = rows.join("\n");
            let row_with = |mark: char| {
                rows.iter()
                    .position(|row| row.contains(mark))
                    .unwrap_or_else(|| panic!("no {mark} row at {width}x{height}: {shown}"))
            };
            assert!(shown.contains("Save this filter as a search"), "{shown}");
            assert!(shown.contains("Name: bad name"), "{shown}");
            assert!(
                shown.contains("A saved search name must be 1 to 40 letters,"),
                "the refusal starts on its own row: {shown}"
            );
            assert!(
                shown.contains("'-' or '_'."),
                "and its tail is drawn too, on the second row: {shown}"
            );
            assert!(!shown.contains('…'), "nothing is cut: {shown}");
            assert_eq!(
                row_with('└') - row_with('┌'),
                5,
                "the box grew by the refusal's second row: {shown}"
            );
            assert!(
                !shown.contains("Esc cancel"),
                "it replaces the hint: {shown}"
            );
        }
        prompt.key(key(KeyCode::Backspace));
        assert_eq!(prompt.refusal(), None, "an edit clears it");
    }

    /// AC-26 (slot 10): nothing panics at sizes smaller than the box.
    #[test]
    fn the_prompt_degrades_at_tiny_sizes() {
        let mut prompt = prompt(&"n".repeat(40));
        prompt.refuse("A saved search named \"x\" already exists.".into());
        for (width, height) in [(0_u16, 24_u16), (1, 24), (20, 3), (3, 2)] {
            let rows = render_lines(width, height, |frame| {
                prompt.render(frame, frame.area(), &Theme::default());
            });
            assert_eq!(rows.len(), usize::from(height), "{width}x{height}");
        }
    }
}
