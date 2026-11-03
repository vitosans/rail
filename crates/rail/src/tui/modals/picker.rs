//! The pickers: `l` toggles any number of the account's labels (`Space` ticks, `Enter`
//! applies the difference from what the threads carried), `m` moves to one (`Enter`), `:theme`
//! and `:accent` preview the highlighted choice on every move (`Enter` keeps it, `Esc` restores).
//! Typing filters with the palette's fuzzy matcher; names are outside text, escaped when drawn.
//! The snooze chooser (`h`) shows each choice's time, and offers a typed time as its first row.

use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use rail_core::config::FieldKeys;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{Command, ModalOutcome, boxed};
use crate::tui::clock::Clock;
use crate::tui::snooze::{SnoozeChoice, ahead, parse_snooze};
use crate::tui::text::{count, escape, input_row, pad, pad_left, width};
use crate::tui::theme::Theme;
use crate::tui::theme::choice::Accent;
use crate::tui::widgets::text_input::TextInput;

/// Rows of items shown.
const SHOWN: usize = 12;
/// The picker's width, its border included.
pub const PICKER_WIDTH: u16 = 60;
/// One swatch colour's cells.
const SWATCH: &str = "██";
/// The theme and accent pickers' hint while the highlight is previewed.
const PREVIEW_HINT: &str = "Enter keeps · Esc restores · ↑↓ move";
/// The theme picker's hint on a terminal that cannot show a scheme.
pub(crate) const NO_PREVIEW_HINT: &str =
    "Enter keeps · Esc cancels · ↑↓ move · no preview without truecolor";
/// The snooze chooser's hint: plain ASCII, as it draws in either icon set.
const SNOOZE_HINT: &str = "Type a time or pick one. Enter snoozes, Esc cancels.";
/// The `id` of the snooze chooser's row that puts snoozed threads back now; a time's row has
/// the time.
pub const RETURN_NOW: i64 = 0;
const RETURN_LABEL: &str = "Return to inbox now";
/// The label of the row offering the last typed time again.
const LAST_LABEL: &str = "Last used";
/// The label of the row a typed time makes.
const TYPED_LABEL: &str = "At";

/// `:accent`'s answer: the scheme's own accent or one of its sixteen slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentAnswer {
    SchemeDefault,
    Slot(u8),
}

impl From<AccentAnswer> for Accent {
    fn from(answer: AccentAnswer) -> Self {
        match answer {
            AccentAnswer::SchemeDefault => Accent::SchemeDefault,
            AccentAnswer::Slot(n) => Accent::Slot(n),
        }
    }
}

/// What a picker chooses, and so what it does with its choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    /// Tick any number of labels.
    Labels,
    /// Move to one label.
    Move,
    /// A colour scheme, previewed as the highlight moves.
    Theme,
    /// An accent slot, previewed as the highlight moves.
    Accent,
    /// The account a new message is written from (`id`: its position).
    Account,
    /// A draft to open in the composer (`id`: the draft's).
    Draft,
    /// When to snooze threads until (`id`: the time, Unix seconds; `RETURN_NOW`: back now).
    Snooze,
    /// A keymap preset (`id`: its position in `KeymapPreset::ALL`).
    Keymap,
    /// Whose remote pictures `I` always loads (`id`: a `TrustChoice`'s).
    Trust,
}

/// What the `I` chooser trusts. Its ids are the picker rows' `id`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustChoice {
    /// The cursor message's sender, by address.
    Sender = 0,
    /// Everyone at the sender's domain, which the receiving server vouched for.
    Domain = 1,
    /// The sender's domain, which the receiving server did not vouch for: only says why not.
    UnvouchedDomain = 2,
}

impl TrustChoice {
    fn from_id(id: i64) -> Option<Self> {
        [Self::Sender, Self::Domain, Self::UnvouchedDomain]
            .into_iter()
            .find(|c| *c as i64 == id)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PickerItem {
    /// A label's folder id; a scheme's index or a slot (`-1` for the default).
    pub id: i64,
    /// Outside text.
    pub name: String,
    pub checked: bool,
    /// Colour cells drawn after the name (a scheme's background, foreground and accent; a
    /// slot's colour).
    pub swatch: Vec<Color>,
}

/// Ids and flags only: a label's name is mail content.
impl std::fmt::Debug for PickerItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerItem")
            .field("id", &self.id)
            .field("checked", &self.checked)
            .field("swatch", &self.swatch.len())
            .finish_non_exhaustive()
    }
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

pub struct Picker {
    kind: PickerKind,
    items: Vec<PickerItem>,
    initial: BTreeSet<i64>,
    input: TextInput,
    matcher: Matcher,
    /// Indices of the matching items, best first.
    shown: Vec<usize>,
    selected: usize,
    /// A `Theme` or `Accent` picker previews its highlight unless told not to.
    preview: bool,
    /// A snooze chooser's clock, title and typed time.
    snooze: Option<SnoozeParts>,
}

/// What a snooze chooser adds to a picker: the clock its times are read and written by, when
/// the threads are snoozed until (its title), and what the typed text reads as (`None` while
/// nothing is typed).
struct SnoozeParts {
    clock: Clock,
    /// How many threads the chooser snoozes: its title counts more than one.
    threads: usize,
    until: Option<i64>,
    reading: Option<Result<i64, &'static str>>,
}

impl SnoozeParts {
    /// A snooze row: the label, then its time right-aligned in the clock's column with a space
    /// before it. The time is muted but on the cursor row, which is `selected` throughout.
    fn line(
        &self,
        label: &str,
        at: Option<i64>,
        selected: bool,
        theme: &Theme,
        cols: usize,
    ) -> Line<'static> {
        let (style, muted) = if selected {
            (theme.selected, theme.selected)
        } else {
            (theme.text, theme.muted)
        };
        let label_cols = cols.saturating_sub(self.clock.ahead_cols() + 1);
        let time = at.map(|at| self.clock.ahead(at)).unwrap_or_default();
        Line::from(vec![
            Span::styled(pad(&escape(label), label_cols), style),
            Span::styled(pad_left(&time, cols.saturating_sub(label_cols)), muted),
        ])
    }
}

impl Picker {
    /// The editing keys of the line, handed over before each key.
    pub(crate) fn use_field_keys(&mut self, keys: FieldKeys) {
        self.input.set_keys(keys);
    }

    /// The items as given, for tests that check which one is marked.
    #[cfg(test)]
    pub fn items(&self) -> &[PickerItem] {
        &self.items
    }

    pub fn new(kind: PickerKind, items: Vec<PickerItem>) -> Self {
        let initial = items.iter().filter(|i| i.checked).map(|i| i.id).collect();
        let shown = (0..items.len()).collect();
        let mut picker = Self {
            kind,
            items,
            initial,
            input: TextInput::default(),
            matcher: Matcher::new(Config::DEFAULT),
            shown,
            selected: 0,
            preview: true,
            snooze: None,
        };
        if matches!(kind, PickerKind::Theme | PickerKind::Accent) {
            // The active choice is highlighted.
            picker.selected = picker.items.iter().position(|i| i.checked).unwrap_or(0);
        }
        picker
    }

    /// The snooze chooser for `threads` threads: "Return to inbox now" first when `snoozed`, then
    /// the last typed time (`last`, unless a choice offers the same), then `choices`. Its title
    /// says until when the one thread is snoozed (`until`), or how many threads it snoozes, and a
    /// typed time is offered first.
    pub fn snooze(
        choices: &[SnoozeChoice],
        clock: Clock,
        (threads, until, snoozed): (usize, Option<i64>, bool),
        last: Option<i64>,
    ) -> Self {
        let back = snoozed.then_some((RETURN_NOW, RETURN_LABEL));
        let last = last
            .filter(|at| choices.iter().all(|c| c.at != *at))
            .map(|at| (at, LAST_LABEL));
        let rows = back
            .into_iter()
            .chain(last)
            .chain(choices.iter().map(|c| (c.at, c.label)));
        let items = rows
            .map(|(id, name)| PickerItem {
                id,
                name: name.to_string(),
                checked: false,
                swatch: Vec::new(),
            })
            .collect();
        let mut picker = Self::new(PickerKind::Snooze, items);
        picker.snooze = Some(SnoozeParts {
            clock,
            threads,
            until,
            reading: None,
        });
        picker
    }

    /// The highlighted row's position among the shown items (tests).
    #[cfg(test)]
    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    /// The items' names, in order (tests).
    #[cfg(test)]
    pub(crate) fn names(&self) -> Vec<&str> {
        self.items.iter().map(|i| i.name.as_str()).collect()
    }

    /// Moves a snooze chooser's clock (tests: a chooser keeps the clock it was opened with).
    #[cfg(test)]
    pub(crate) fn set_now(&mut self, now: i64) {
        if let Some(parts) = self.snooze.as_mut() {
            parts.clock.set_now(now);
        }
    }

    /// Moving the highlight shows nothing, and the hint says why (a theme on a terminal
    /// without truecolor).
    pub fn without_preview(mut self) -> Self {
        self.preview = false;
        self
    }

    fn previews(&self) -> bool {
        self.preview && matches!(self.kind, PickerKind::Theme | PickerKind::Accent)
    }

    fn refilter(&mut self) {
        let query = self.input.value();
        self.shown = if query.trim().is_empty() {
            (0..self.items.len()).collect()
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let candidates = self.items.iter().enumerate().map(|(index, i)| Candidate {
                index,
                text: &i.name,
            });
            pattern
                .match_list(candidates, &mut self.matcher)
                .into_iter()
                .map(|(candidate, _)| candidate.index)
                .collect()
        };
        if let Some(parts) = self.snooze.as_mut() {
            parts.reading = (!query.trim().is_empty()).then(|| parse_snooze(query, &parts.clock));
        }
        self.selected = 0;
    }

    /// The typed time, when a snooze chooser's text reads as one: its row comes first, unless a
    /// choice still shown already offers that time (`tomorrow` typed is the Tomorrow row).
    fn typed(&self) -> Option<i64> {
        let at = self.snooze.as_ref()?.reading?.ok()?;
        let offered = self
            .shown
            .iter()
            .any(|&i| self.items.get(i).is_some_and(|c| c.id == at));
        (!offered).then_some(at)
    }

    /// The rows before the matching items: the typed time's, if any.
    fn lead(&self) -> usize {
        usize::from(self.typed().is_some())
    }

    /// The highlighted item's command, for the kinds that choose one item.
    fn choice(&self) -> Option<Command> {
        if let Some(at) = self.typed().filter(|_| self.selected == 0) {
            let said = Some(self.input.value().trim().to_string());
            return Some(Command::Snooze { until: at, said });
        }
        let item = self
            .shown
            .get(self.selected.checked_sub(self.lead())?)
            .and_then(|&i| self.items.get(i))?;
        match self.kind {
            PickerKind::Labels => None,
            PickerKind::Move => Some(Command::MoveTo(item.id)),
            PickerKind::Theme => Some(Command::Theme(usize::try_from(item.id).ok())),
            PickerKind::Accent => Some(Command::Accent(
                u8::try_from(item.id)
                    .ok()
                    .filter(|n| *n <= 15)
                    .map_or(AccentAnswer::SchemeDefault, AccentAnswer::Slot),
            )),
            PickerKind::Account => usize::try_from(item.id).ok().map(Command::Account),
            PickerKind::Draft => usize::try_from(item.id).ok().map(Command::DraftRow),
            PickerKind::Keymap => usize::try_from(item.id).ok().map(Command::Keymap),
            PickerKind::Trust => TrustChoice::from_id(item.id).map(Command::Trust),
            PickerKind::Snooze if item.id == RETURN_NOW => Some(Command::SnoozeReturn),
            PickerKind::Snooze => Some(Command::Snooze {
                until: item.id,
                said: None,
            }),
        }
    }

    fn enter(&mut self) -> ModalOutcome {
        if self.kind != PickerKind::Labels {
            return match self.choice() {
                Some(command) => self.still_ahead(command),
                None => ModalOutcome::Stay,
            };
        }
        let now: BTreeSet<i64> = self
            .items
            .iter()
            .filter(|i| i.checked)
            .map(|i| i.id)
            .collect();
        ModalOutcome::Run(Command::Labels {
            add: now.difference(&self.initial).copied().collect(),
            remove: self.initial.difference(&now).copied().collect(),
        })
    }

    /// A snooze's time asked again at Enter, since the clock has moved on since it was shown: the
    /// time shown is sent, unless it has become too soon. Then the chooser says why and shows no
    /// rows, so a second Enter sends nothing until the text changes.
    fn still_ahead(&mut self, command: Command) -> ModalOutcome {
        let (Command::Snooze { until, .. }, Some(parts)) = (&command, self.snooze.as_mut()) else {
            return ModalOutcome::Run(command);
        };
        match ahead(*until, parts.clock.now()) {
            Ok(_) => ModalOutcome::Run(command),
            Err(why) => {
                parts.reading = Some(Err(why));
                self.shown.clear();
                self.selected = 0;
                ModalOutcome::Stay
            }
        }
    }

    /// After the highlight may have moved: a preview of it in the kinds that preview.
    fn moved(&self) -> ModalOutcome {
        if !self.previews() {
            return ModalOutcome::Stay;
        }
        self.choice()
            .map_or(ModalOutcome::Stay, ModalOutcome::Preview)
    }

    fn step(&mut self, down: bool) -> ModalOutcome {
        self.selected = if down {
            (self.selected + 1).min((self.lead() + self.shown.len()).saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
        self.moved()
    }

    /// `Down`/`Up` and `Ctrl-n`/`Ctrl-p` move (typed letters filter, so `j` and `k` cannot).
    pub(super) fn key(&mut self, key: KeyEvent) -> ModalOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc if !self.input.takes_esc() => ModalOutcome::Close,
            KeyCode::Enter => self.enter(),
            KeyCode::Down => self.step(true),
            KeyCode::Up => self.step(false),
            KeyCode::Char('n') if ctrl => self.step(true),
            KeyCode::Char('p') if ctrl => self.step(false),
            KeyCode::Char(' ') if self.kind == PickerKind::Labels => {
                if let Some(item) = self
                    .shown
                    .get(self.selected)
                    .and_then(|&i| self.items.get_mut(i))
                {
                    item.checked = !item.checked;
                }
                ModalOutcome::Stay
            }
            _ => {
                if self.input.handle_key(key) {
                    self.refilter();
                    return self.moved();
                }
                ModalOutcome::Stay
            }
        }
    }

    /// The title, the hint row and the line shown when nothing matches.
    pub(crate) fn words(&self) -> (&'static str, &'static str, &'static str) {
        let hint = if self.preview {
            PREVIEW_HINT
        } else {
            NO_PREVIEW_HINT
        };
        match self.kind {
            PickerKind::Labels => (
                "Labels",
                "Space ticks · Enter applies · Esc cancels · ↑↓ move",
                "No label matches.",
            ),
            PickerKind::Move => (
                "Move to",
                "Enter moves · Esc cancels · ↑↓ move",
                "No label matches.",
            ),
            PickerKind::Theme => ("Theme", hint, "No theme matches."),
            PickerKind::Accent => ("Accent", hint, "No accent matches."),
            PickerKind::Account => (
                "Write from",
                "Enter writes from it · Esc cancels · ↑↓ move",
                "No account matches.",
            ),
            PickerKind::Draft => (
                "Drafts",
                "Enter opens · Esc cancels · ↑↓ move",
                "No draft matches.",
            ),
            PickerKind::Keymap => (
                "Keys",
                "Enter switches · Esc keeps · ↑↓ move",
                "No preset matches.",
            ),
            PickerKind::Trust => (
                "Always load remote pictures from",
                "Enter trusts · Esc cancels · ↑↓ move",
                "Nothing matches.",
            ),
            PickerKind::Snooze => (
                "Snooze until",
                SNOOZE_HINT,
                self.snooze
                    .as_ref()
                    .and_then(|s| s.reading)
                    .and_then(Result::err)
                    .unwrap_or("No time matches."),
            ),
        }
    }

    /// One item's row: the mark and the escaped name, padded, then the swatch cells.
    fn row(&self, item: &PickerItem, style: Style, cols: usize) -> Line<'static> {
        let mark = match (self.kind, item.checked) {
            (PickerKind::Labels, true) => "[x] ",
            (PickerKind::Labels, false) => "[ ] ",
            _ => "",
        };
        let swatch_width = SWATCH.chars().count() * item.swatch.len();
        let text = pad(
            &format!("{mark}{}", escape(&item.name)),
            cols.saturating_sub(swatch_width),
        );
        let mut spans = vec![Span::styled(text, style)];
        if swatch_width <= cols {
            spans.extend(
                item.swatch
                    .iter()
                    .map(|&c| Span::styled(SWATCH, style.fg(c))),
            );
        }
        Line::from(spans)
    }

    /// The box's title: the kind's; a snooze chooser's says until when its one thread is
    /// snoozed, or how many threads it snoozes ("Snooze 120 threads until").
    fn title(&self) -> String {
        match &self.snooze {
            Some(SnoozeParts {
                until: Some(until),
                clock,
                ..
            }) => format!("Snoozed until {}", clock.long(*until)),
            Some(s) if s.threads > 1 => format!("Snooze {} threads until", count(s.threads)),
            _ => self.words().0.to_string(),
        }
    }

    /// Row `row`: the typed time's first, then the matching items.
    fn line(&self, row: usize, theme: &Theme, cols: usize) -> Option<Line<'static>> {
        let selected = row == self.selected;
        let Some(index) = row.checked_sub(self.lead()) else {
            let parts = self.snooze.as_ref()?;
            return Some(parts.line(TYPED_LABEL, self.typed(), selected, theme, cols));
        };
        let item = self.items.get(*self.shown.get(index)?)?;
        if let Some(parts) = &self.snooze {
            let at = (item.id != RETURN_NOW).then_some(item.id);
            return Some(parts.line(&item.name, at, selected, theme, cols));
        }
        let style = if selected { theme.selected } else { theme.text };
        Some(self.row(item, style, cols))
    }

    pub(super) fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let (_, hint, empty) = self.words();
        let title = self.title();
        let height = u16::try_from(SHOWN + 5).unwrap_or(u16::MAX);
        // Wide enough for the hint and its border.
        let hint_width = u16::try_from(width(hint) + 2).unwrap_or(u16::MAX);
        let inner = boxed(
            frame,
            area,
            PICKER_WIDTH.max(hint_width),
            height,
            &title,
            theme,
        );
        let cols = usize::from(inner.width);
        let (typed, column) = input_row("> ", self.input.value(), self.input.cursor(), cols);
        let mut lines = vec![Line::styled(typed, theme.accent), Line::default()];
        let first = (self.selected + 1).saturating_sub(SHOWN);
        let rows = self.lead() + self.shown.len();
        lines.extend(
            (first..rows)
                .take(SHOWN)
                .filter_map(|row| self.line(row, theme, cols)),
        );
        if rows == 0 {
            lines.push(Line::styled(empty, theme.muted));
        }
        // The hint stays on the last inside row however many items there are.
        let used = lines.len();
        let last = usize::from(inner.height).saturating_sub(1);
        lines.extend((used..last).map(|_| Line::default()));
        lines.push(Line::styled(pad(hint, cols), theme.muted));
        frame.render_widget(Paragraph::new(lines), inner);
        if cols > 0 && inner.height > 0 {
            let x = inner
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            frame.set_cursor_position(Position::new(x, inner.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(p: &mut Picker, code: KeyCode) -> ModalOutcome {
        p.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn debug_shows_ids_and_flags_not_the_label_name() {
        let shown = format!("{:?}", items()[0]);
        assert!(!shown.contains("Projects"), "{shown}");
        assert!(
            shown.contains("id: 1") && shown.contains("checked: true"),
            "{shown}"
        );
    }

    fn items() -> Vec<PickerItem> {
        vec![
            PickerItem {
                id: 1,
                name: "Projects".into(),
                checked: true,
                swatch: Vec::new(),
            },
            PickerItem {
                id: 2,
                name: "Receipts".into(),
                checked: false,
                swatch: Vec::new(),
            },
            PickerItem {
                id: 3,
                name: "Travel".into(),
                checked: false,
                swatch: Vec::new(),
            },
        ]
    }

    #[test]
    fn toggle_mode_applies_what_changed_relative_to_the_start() {
        let mut p = Picker::new(PickerKind::Labels, items());
        press(&mut p, KeyCode::Char(' ')); // Projects off
        for c in "rec".chars() {
            press(&mut p, KeyCode::Char(c));
        }
        press(&mut p, KeyCode::Char(' ')); // Receipts on
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::Labels {
                add: vec![2],
                remove: vec![1]
            })
        );
    }

    #[test]
    fn move_mode_moves_to_the_highlighted_label_and_esc_cancels() {
        let mut p = Picker::new(PickerKind::Move, items());
        press(&mut p, KeyCode::Down);
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::MoveTo(2))
        );
        assert_eq!(press(&mut p, KeyCode::Esc), ModalOutcome::Close);
        let mut none = Picker::new(PickerKind::Move, items());
        for c in "zzz".chars() {
            press(&mut none, KeyCode::Char(c));
        }
        assert_eq!(
            press(&mut none, KeyCode::Enter),
            ModalOutcome::Stay,
            "nothing matches"
        );
    }

    #[test]
    fn ctrl_n_and_ctrl_p_move_while_letters_filter() {
        let mut p = Picker::new(PickerKind::Move, items());
        p.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        p.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::MoveTo(3))
        );
        p.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::MoveTo(2))
        );
        press(&mut p, KeyCode::Char('j'));
        assert_eq!(p.input.value(), "j", "j is typed");
    }

    fn themes() -> Vec<PickerItem> {
        ["Rail default", "Dracula", "Nord"]
            .iter()
            .enumerate()
            .map(|(i, name)| PickerItem {
                id: i64::try_from(i).unwrap() - 1,
                name: (*name).into(),
                checked: i == 1,
                swatch: vec![Color::Black, Color::White, Color::Cyan],
            })
            .collect()
    }

    #[test]
    fn theme_kind_previews_on_every_move_and_enters_the_highlighted_one() {
        let mut p = Picker::new(PickerKind::Theme, themes());
        assert_eq!(p.selected_index(), 1, "opens on the checked item");
        assert_eq!(
            press(&mut p, KeyCode::Down),
            ModalOutcome::Preview(Command::Theme(Some(1)))
        );
        assert_eq!(
            press(&mut p, KeyCode::Up),
            ModalOutcome::Preview(Command::Theme(Some(0)))
        );
        assert_eq!(
            press(&mut p, KeyCode::Up),
            ModalOutcome::Preview(Command::Theme(None))
        );
        for c in "nor".chars() {
            press(&mut p, KeyCode::Char(c));
        }
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::Theme(Some(1)))
        );
    }

    #[test]
    fn filtering_previews_the_new_top_match_and_esc_closes() {
        let mut p = Picker::new(PickerKind::Theme, themes());
        let outcome = press(&mut p, KeyCode::Char('n'));
        assert_eq!(outcome, ModalOutcome::Preview(Command::Theme(Some(1))));
        assert_eq!(press(&mut p, KeyCode::Esc), ModalOutcome::Close);
    }

    #[test]
    fn accent_kind_carries_the_slot_and_the_default() {
        let items: Vec<PickerItem> = (-1..16)
            .map(|n| PickerItem {
                id: n,
                name: if n < 0 {
                    "scheme default".into()
                } else {
                    format!("{n}")
                },
                checked: n == -1,
                swatch: vec![],
            })
            .collect();
        let mut p = Picker::new(PickerKind::Accent, items);
        assert_eq!(
            press(&mut p, KeyCode::Down),
            ModalOutcome::Preview(Command::Accent(AccentAnswer::Slot(0)))
        );
        assert_eq!(
            press(&mut p, KeyCode::Up),
            ModalOutcome::Preview(Command::Accent(AccentAnswer::SchemeDefault))
        );
    }

    #[test]
    fn a_theme_picker_without_preview_only_moves_and_says_so() {
        let mut p = Picker::new(PickerKind::Theme, themes()).without_preview();
        assert_eq!(p.selected_index(), 1, "still opens on the checked item");
        assert_eq!(press(&mut p, KeyCode::Down), ModalOutcome::Stay);
        assert_eq!(press(&mut p, KeyCode::Char('d')), ModalOutcome::Stay);
        assert_eq!(press(&mut p, KeyCode::Char('r')), ModalOutcome::Stay);
        assert_eq!(p.words().1, NO_PREVIEW_HINT);
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            ModalOutcome::Run(Command::Theme(Some(0)))
        );
    }
}
