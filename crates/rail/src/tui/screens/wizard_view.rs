//! Drawing the wizard screen: the step list on the left, the current step on the right, a
//! failure panel when something went wrong, and the status line at the bottom.

use rail_core::onboarding::{Body, FieldView, Primary, Step, StepState};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::wizard::{Mode, WizardScreen};
use crate::tui::keys::Action;
use crate::tui::theme::Theme;
use crate::tui::widgets::select::Checkbox;
use crate::tui::widgets::status_line::StatusLine;
use crate::tui::widgets::step_list::{StepList, StepMark};
use crate::tui::widgets::text_input::{FIELD_HEIGHT, TextField};

const STEPS_WIDTH: u16 = 18;
const FAILURE_HEIGHT: u16 = 10;
const STATUS_HEIGHT: u16 = 2;
/// The status message while the account is written; nothing can cancel it.
const SAVING: &str = "Saving… please wait";

impl WizardScreen {
    pub(super) fn render(&self, frame: &mut Frame, theme: &Theme) {
        let [main, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)])
                .areas(frame.area());
        let [steps, content] =
            Layout::horizontal([Constraint::Length(STEPS_WIDTH), Constraint::Min(0)]).areas(main);
        self.render_steps(frame, steps, theme);
        self.render_content(frame, content, theme);
        self.render_status(frame, status, theme);
    }

    fn render_steps(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::new()
            .borders(Borders::RIGHT)
            .title(Line::styled(" Rail setup ", theme.accent));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let marks: Vec<(&str, StepMark)> = self
            .model
            .steps
            .iter()
            .map(|(label, state)| (*label, mark(*state)))
            .collect();
        frame.render_widget(StepList::new(&marks, theme), inner.inner(Margin::new(1, 1)));
    }

    fn render_content(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let failure = self.failure_lines(theme);
        let failure_height = if failure.is_empty() {
            0
        } else {
            FAILURE_HEIGHT
        };
        let [title, body, failure_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(failure_height),
        ])
        .areas(area.inner(Margin::new(2, 1)));
        let heading = Line::styled(self.model.title.as_str(), theme.accent);
        frame.render_widget(Paragraph::new(heading), title);
        self.render_body(frame, body, theme);
        let failure = Paragraph::new(failure).wrap(Wrap { trim: false });
        frame.render_widget(failure, failure_area);
    }

    /// The wizard's failure with its way out, or else a failure only the screen knows of.
    fn failure_lines<'a>(&'a self, theme: &Theme) -> Vec<Line<'a>> {
        let Some(view) = &self.model.failure else {
            return self
                .failure
                .iter()
                .map(|text| Line::styled(text.as_str(), theme.error))
                .collect();
        };
        let mut lines = vec![Line::styled(view.text.as_str(), theme.error)];
        match &view.primary {
            Primary::Retry => lines.push(Line::styled(
                format!("Press {} to retry.", self.keymap.label(Action::WizardRetry)),
                theme.accent,
            )),
            Primary::CopyUrl(url) => {
                lines.push(Line::styled(url.as_str(), theme.accent));
                lines.push(Line::styled(
                    format!(
                        "Press {} to copy this URL, or {} to retry once it is approved.",
                        self.keymap.label(Action::WizardCopyUrl),
                        self.keymap.label(Action::WizardRetry)
                    ),
                    theme.muted,
                ));
            }
        }
        lines
    }

    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match &self.model.body {
            Body::Choices(_) => frame.render_widget(self.select.widget(theme, true), area),
            Body::Presets { .. } => self.render_presets(frame, area, theme),
            Body::Instructions { .. } => self.render_instructions(frame, area, theme),
            Body::Fields(fields) => self.render_fields(frame, area, theme, fields),
            Body::Message(text) => self.render_message(frame, area, theme, text),
            Body::Report { .. } => self.render_report(frame, area, theme),
        }
    }

    fn render_presets(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Body::Presets {
            problems, source, ..
        } = &self.model.body
        else {
            return;
        };
        let list_height = u16::try_from(self.select.items().len().max(1)).unwrap_or(u16::MAX);
        let problem_height = u16::try_from(problems.len()).unwrap_or(u16::MAX);
        let [list, problem_area, _, field] = Layout::vertical([
            Constraint::Length(list_height),
            Constraint::Length(problem_height),
            Constraint::Length(1),
            Constraint::Length(FIELD_HEIGHT),
        ])
        .areas(area);
        if self.select.items().is_empty() {
            let none = Line::styled("No presets found.", theme.muted);
            frame.render_widget(Paragraph::new(none), list);
        } else {
            frame.render_widget(self.select.widget(theme, self.focus == 0), list);
        }
        let lines: Vec<Line> = problems
            .iter()
            .map(|p| Line::styled(p.as_str(), theme.error))
            .collect();
        frame.render_widget(Paragraph::new(lines), problem_area);
        self.render_field(frame, field, theme, source, self.focus == 1);
    }

    /// Also records the panel's size, so the scroll keys count rows as they are drawn.
    fn render_instructions(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Body::Instructions { text, portal } = &self.model.body else {
            return;
        };
        let [panel, link] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(area);
        self.panel
            .set((panel.width.saturating_sub(1), panel.height));
        frame.render_widget(self.scroll.widget(text, theme), panel);
        let portal = Line::styled(format!("Portal: {portal}"), theme.muted);
        frame.render_widget(Paragraph::new(portal).wrap(Wrap { trim: false }), link);
    }

    fn render_fields(&self, frame: &mut Frame, area: Rect, theme: &Theme, fields: &[FieldView]) {
        let max_visible = usize::from(area.height / FIELD_HEIGHT);
        let start_index = if fields.len() <= max_visible || max_visible == 0 {
            0
        } else if self.focus >= max_visible {
            self.focus + 1 - max_visible
        } else {
            0
        };

        for (slot, (index, view)) in fields.iter().enumerate().skip(start_index).enumerate() {
            let offset = u16::try_from(slot)
                .unwrap_or(u16::MAX)
                .saturating_mul(FIELD_HEIGHT);
            if offset.saturating_add(FIELD_HEIGHT) > area.height {
                break;
            }
            let row = Rect::new(area.x, area.y + offset, area.width, FIELD_HEIGHT);
            self.render_field(frame, row, theme, view, index == self.focus);
        }
    }

    fn render_field(
        &self,
        frame: &mut Frame,
        row: Rect,
        theme: &Theme,
        view: &FieldView,
        focused: bool,
    ) {
        if let Some(checked) = view.checkbox {
            let checkbox = Checkbox {
                label: view.label,
                checked,
                focused,
                theme,
            };
            frame.render_widget(checkbox, row);
            return;
        }
        let Some(input) = self.inputs.get(&view.field) else {
            return;
        };
        let field = TextField {
            label: view.label,
            input,
            focused,
            error: view.error.as_deref(),
            hint: view.hint.as_deref(),
            theme,
        };
        if focused {
            frame.set_cursor_position(field.cursor_position(row));
        }
        frame.render_widget(field, row);
    }

    fn render_message(&self, frame: &mut Frame, area: Rect, theme: &Theme, text: &str) {
        let mut lines = vec![Line::styled(text, theme.text), Line::default()];
        if let Some(url) = &self.model.url {
            lines.push(Line::styled(
                "If the browser did not open, visit:",
                theme.muted,
            ));
            lines.push(Line::styled(url.as_str(), theme.accent));
            if self.wizard.step() == Step::Login {
                lines.push(Line::styled(
                    format!(
                        "Press {} to copy this URL.",
                        self.keymap.label(Action::WizardCopyUrl)
                    ),
                    theme.muted,
                ));
            }
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }

    fn render_report(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Body::Report { lines, .. } = &self.model.body else {
            return;
        };
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let [table, _, choices] = Layout::vertical([
            Constraint::Length(height),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);
        let rows: Vec<Line> = lines
            .iter()
            .map(|(label, value)| Line::styled(format!("{label:<10} {value}"), theme.text))
            .collect();
        frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), table);
        frame.render_widget(self.select.widget(theme, true), choices);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let message = if self.saving() {
            Some(SAVING)
        } else {
            self.notice
                .as_deref()
                .or(self.model.notice.as_deref())
                .or(self.model.busy.as_deref())
        };
        let hints = self.hints();
        let status = StatusLine {
            message,
            spinner: self.model.busy.as_ref().map(|_| self.tick),
            hints: &hints,
            theme,
        };
        frame.render_widget(status, area);
    }

    fn hints(&self) -> String {
        if self.saving() {
            return "Saving the account · Ctrl-C quits once it is saved".to_string();
        }
        let k = |action: Action| self.keymap.label(action);
        let checkbox = self.focused_field().is_some_and(|f| f.checkbox.is_some());
        match self.mode() {
            Mode::Choices => format!(
                "{}/{} move · {} choose · {} back · {} quit",
                k(Action::WizardUp),
                k(Action::WizardDown),
                k(Action::WizardNext),
                k(Action::WizardBack),
                k(Action::GlobalQuit)
            ),
            Mode::Fields if checkbox => format!(
                "{} toggle · {} next field · {} continue · {} back",
                k(Action::WizardToggle),
                k(Action::WizardNextField),
                k(Action::WizardNext),
                k(Action::WizardBack)
            ),
            Mode::Fields => format!(
                "{} next field · {} continue · {} back · Ctrl-C quit",
                k(Action::WizardNextField),
                k(Action::WizardNext),
                k(Action::WizardBack)
            ),
            Mode::Instructions(_) => format!(
                "{}/{} scroll · {} open the portal · {} continue · {} back",
                k(Action::WizardUp),
                k(Action::WizardDown),
                k(Action::WizardOpenPortal),
                k(Action::WizardNext),
                k(Action::WizardBack)
            ),
            Mode::Message => format!(
                "{} continue · {} retry · {} back · {} quit",
                k(Action::WizardNext),
                k(Action::WizardRetry),
                k(Action::WizardBack),
                k(Action::GlobalQuit)
            ),
        }
    }
}

fn mark(state: StepState) -> StepMark {
    match state {
        StepState::Done => StepMark::Done,
        StepState::Current => StepMark::Current,
        StepState::Upcoming => StepMark::Upcoming,
        StepState::Skipped => StepMark::Skipped,
    }
}

#[cfg(test)]
mod tests;
