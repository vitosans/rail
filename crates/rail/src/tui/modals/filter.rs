//! The `/` filter line: the text, edited in place, and the operators Rail reads from it.

use crossterm::event::{KeyCode, KeyEvent};
use rail_core::config::FieldKeys;
use rail_core::store::filter::{MAX_FILTER_CHARS, parse};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::{Command, ModalOutcome, paint_background};
use crate::tui::text::{escape, fit, input_row, width};
use crate::tui::theme::Theme;
use crate::tui::widgets::text_input::TextInput;

pub struct FilterLine {
    input: TextInput,
}

impl FilterLine {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    pub fn new(text: &str) -> Self {
        let mut input = TextInput::default();
        input.set_value(text);
        Self { input }
    }

    /// The operators as Rail reads them, joined.
    pub fn parsed(&self) -> String {
        parse(self.input.value()).describe().join(" · ")
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> ModalOutcome {
        match key.code {
            KeyCode::Enter => ModalOutcome::Close,
            KeyCode::Esc => ModalOutcome::Run(Command::ClearFilter),
            _ => {
                let before = self.input.clone();
                if !self.input.handle_key(key) {
                    return ModalOutcome::Stay;
                }
                if self.input.value().chars().count() > MAX_FILTER_CHARS {
                    self.input = before;
                    return ModalOutcome::Stay;
                }
                ModalOutcome::Typed(self.input.value().to_string())
            }
        }
    }

    /// Drawn in the first row of `area`: `/text` with the cursor in it and, muted, the parsed
    /// operators.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let row = Rect::new(area.x, area.y, area.width, 1);
        let cols = usize::from(area.width);
        let (typed, column) = input_row("/", self.input.value(), self.input.cursor(), cols);
        let parsed = format!("   {}", escape(&self.parsed()));
        let room = cols.saturating_sub(width(&typed));
        let line = Line::from(vec![
            Span::styled(typed, theme.accent),
            Span::styled(fit(&parsed, room), theme.muted),
        ]);
        frame.render_widget(Clear, row);
        paint_background(frame, row, theme);
        frame.render_widget(Paragraph::new(line), row);
        let x = row
            .x
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
        frame.set_cursor_position(Position::new(x, row.y));
    }
}
