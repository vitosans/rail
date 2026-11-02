//! The hint bar: key hints for the focused pane, built from the keymap, a notice in their
//! place while one shows, and each account's state on the right. One row when stacked; in split
//! mode three, each key in its own box with the words and the accounts on the middle row. With
//! the hints off (`hints = false`, `:hints`) one row of the notice and the accounts.

use rail_core::config::Icons;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::layout::{HINT_ROWS_BOXED, border_set};
use super::status::account_part;
use super::view::Glyphs;
use super::{Focus, MailScreen};
use crate::tui::keys::{Action, Keymap};
use crate::tui::screens::compose::Composer;
use crate::tui::screens::compose::review::DISCARD_QUESTION;
use crate::tui::text::{escape, fit, width};
use crate::tui::theme::Theme;

/// Which hint set the bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pane {
    List,
    Drafts,
    Sidebar,
    Reader,
    Compose,
    /// A compose prompt (headers or the attach path) is open and takes every key.
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Hint {
    pub key: String,
    pub word: String,
}

/// Between two hints.
const SEP: &str = "  ";
/// Between a word and the next key's box: the box's line already sets them apart.
const BOXED_SEP: &str = " ";
/// Between two accounts on the right.
const ACCOUNT_SEP: &str = " · ";

fn hint(key: String, word: &str) -> Hint {
    Hint {
        key,
        word: word.to_string(),
    }
}

/// `j/k`: the two actions' first keys.
fn pair(keymap: &Keymap, a: Action, b: Action) -> String {
    format!("{}/{}", keymap.label(a), keymap.label(b))
}

/// A compose prompt's own keys: they are not keymap actions.
fn prompt_hints() -> Vec<Hint> {
    vec![
        hint("Enter".to_string(), "accept"),
        hint("Tab".to_string(), "complete"),
        hint("Esc".to_string(), "cancel"),
    ]
}

/// The compose review's keys; `provider` is where `s` saves.
fn compose_hints(keymap: &Keymap, provider: &str) -> Vec<Hint> {
    let l = |a: Action| keymap.label(a);
    vec![
        hint(l(Action::ComposeSend), "send"),
        hint(l(Action::ComposePush), &format!("save to {provider}")),
        hint(l(Action::ComposeEdit), "edit"),
        hint(l(Action::ComposeHeaders), "headers"),
        hint(l(Action::ComposeAttach), "attach"),
        hint(l(Action::ComposeRemove), "remove"),
        hint(l(Action::ComposeClose), "keep"),
        hint(l(Action::ComposeDiscard), "discard"),
    ]
}

/// The hints of `pane`, `? help` last; `provider` is where the composer's `s` saves. A prompt's
/// keys are its own, not keymap actions, and `?` would type into it, so it has no `? help`.
pub(super) fn hints(pane: Pane, keymap: &Keymap, sidebar_shown: bool, provider: &str) -> Vec<Hint> {
    let l = |a: Action| keymap.label(a);
    let mut out = match pane {
        Pane::List => vec![
            hint(l(Action::ListOpen), "open"),
            hint(pair(keymap, Action::ListDown, Action::ListUp), "move"),
            hint(l(Action::SidebarFocus), "sidebar"),
            hint(l(Action::ListReply), "reply"),
            hint(l(Action::ListCompose), "compose"),
            hint(l(Action::ListArchive), "archive"),
            hint(l(Action::ListTrash), "trash"),
            hint(l(Action::ListFilter), "filter"),
        ],
        Pane::Drafts => vec![
            hint(l(Action::ListOpen), "open"),
            hint(l(Action::ListTrash), "discard"),
            hint(l(Action::SidebarFocus), "sidebar"),
        ],
        Pane::Sidebar => vec![
            hint(l(Action::SidebarOpen), "open"),
            hint(l(Action::SidebarFold), "fold"),
            hint(pair(keymap, Action::SidebarDown, Action::SidebarUp), "move"),
            hint(l(Action::SidebarFocus), "list"),
        ],
        Pane::Reader => vec![
            hint(
                pair(keymap, Action::ReaderNext, Action::ReaderPrev),
                "message",
            ),
            hint(l(Action::ReaderExpand), "fold"),
            hint(l(Action::ReaderHeaders), "headers"),
            hint(l(Action::ListReply), "reply"),
            hint(l(Action::ListArchive), "archive"),
            hint(l(Action::ReaderBack), "back"),
        ],
        Pane::Prompt => return prompt_hints(),
        Pane::Compose => compose_hints(keymap, provider),
    };
    if !sidebar_shown && matches!(pane, Pane::List | Pane::Drafts) {
        out.retain(|h| h.word != "sidebar");
    }
    out.push(hint(l(Action::GlobalHelp), "help"));
    out
}

/// The cells around a key in its box: a line and a space of padding on each side.
const KEY_BOX_EXTRA: usize = 4;

/// One hint's cells: its key (boxed or not), a space, its word.
fn one_width(h: &Hint, boxed: bool) -> usize {
    let frame = if boxed { KEY_BOX_EXTRA } else { 0 };
    width(&h.key) + frame + 1 + width(&h.word)
}

/// What goes between two hints.
fn sep(boxed: bool) -> &'static str {
    if boxed { BOXED_SEP } else { SEP }
}

/// The cells `hints` take, separators included.
#[cfg(test)]
pub(super) fn hint_width(hints: &[Hint], boxed: bool) -> usize {
    let text: usize = hints.iter().map(|h| one_width(h, boxed)).sum();
    text + width(sep(boxed)) * hints.len().saturating_sub(1)
}

/// The hints that fit in `room`: whole hints from the left, the last one (`? help`) always.
/// `boxed` counts each key's box.
pub(super) fn fit_hints(hints: &[Hint], boxed: bool, room: usize) -> Vec<Hint> {
    let Some((help, rest)) = hints.split_last() else {
        return Vec::new();
    };
    let mut kept: Vec<Hint> = Vec::new();
    let mut used = one_width(help, boxed);
    for h in rest {
        let next = used + one_width(h, boxed) + width(sep(boxed));
        if next > room {
            break;
        }
        used = next;
        kept.push(h.clone());
    }
    kept.push(help.clone());
    kept
}

/// What the bar draws.
pub(super) struct Bar<'a> {
    pub hints: Vec<Hint>,
    /// `hints = false` (or `:hints`): the left part keeps only the notice.
    pub show_hints: bool,
    /// A notice in place of the hints, and whether it is an error.
    pub notice: Option<(&'a str, bool)>,
    /// The glyph before each account's state.
    pub dot: &'static str,
    /// Each account's dot colour and (escaped) state text.
    pub right: Vec<(Color, String)>,
}

impl Bar<'_> {
    /// The accounts joined, as one string.
    fn right_text(&self) -> String {
        let parts: Vec<String> = self
            .right
            .iter()
            .map(|(_, text)| format!("{} {text}", self.dot))
            .collect();
        parts.join(ACCOUNT_SEP)
    }
}

/// The right part: coloured per account when it fits whole, else cut as one muted string.
fn right_spans(bar: &Bar<'_>, theme: &Theme, room: usize) -> Vec<Span<'static>> {
    let whole = bar.right_text();
    if width(&whole) > room {
        return vec![Span::styled(fit(&whole, room), theme.muted)];
    }
    let mut spans = Vec::new();
    for (i, (color, text)) in bar.right.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(ACCOUNT_SEP, theme.muted));
        }
        spans.push(Span::styled(
            format!("{} ", bar.dot),
            theme.muted.fg(*color),
        ));
        spans.push(Span::styled(text.clone(), theme.muted));
    }
    spans
}

/// The left part: the notice, or the hints that fit in `room` (nothing with the hints off).
fn left_spans(bar: &Bar<'_>, theme: &Theme, room: usize) -> Vec<Span<'static>> {
    if let Some((text, error)) = bar.notice {
        let style = if error { theme.error } else { theme.muted };
        return vec![Span::styled(fit(text, room), style)];
    }
    if !bar.show_hints {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for (i, h) in fit_hints(&bar.hints, false, room).iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(SEP));
        }
        spans.push(Span::styled(h.key.clone(), theme.selected));
        spans.push(Span::styled(format!(" {}", h.word), theme.muted));
    }
    spans
}

/// Draw the bar: the left part, then the accounts at the right edge. The accounts give way
/// first, down to what leaves room for `? help`.
/// With three rows or more, each key is drawn in its own box (`icons` picks the lines). With
/// the hints off nothing is kept for `? help`, and no box is drawn.
pub(super) fn render_hints(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    bar: &Bar<'_>,
    icons: Icons,
) {
    let boxed = bar.show_hints && area.height >= HINT_ROWS_BOXED;
    let cols = usize::from(area.width);
    let help_w = match bar.hints.last() {
        Some(h) if bar.show_hints => one_width(h, boxed),
        _ => 0,
    };
    let right_room = cols.saturating_sub(help_w + width(SEP));
    let right = right_spans(bar, theme, width(&bar.right_text()).min(right_room));
    let right_w = spans_width(&right);
    let gap = if right_w > 0 { width(SEP) } else { 0 };
    let room = cols.saturating_sub(right_w + gap);
    let [top, mut mid, bottom] = if boxed {
        boxed_left(bar, theme, room, &border_set(icons))
    } else {
        [Vec::new(), left_spans(bar, theme, room), Vec::new()]
    };
    let used = spans_width(&mid);
    mid.push(Span::raw(" ".repeat(cols.saturating_sub(used + right_w))));
    mid.extend(right);
    let lines = if boxed {
        vec![Line::from(top), Line::from(mid), Line::from(bottom)]
    } else {
        vec![Line::from(mid)]
    };
    frame.render_widget(Paragraph::new(lines), area);
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| width(&s.content)).sum()
}

/// A key's box as its top, middle and bottom segments: lines in `border`, the key in `accent`.
fn key_box(key: &str, set: &border::Set<'_>, theme: &Theme) -> [Vec<Span<'static>>; 3] {
    let line = |h: &str| h.repeat(width(key) + 2);
    let top = format!(
        "{}{}{}",
        set.top_left,
        line(set.horizontal_top),
        set.top_right
    );
    let bottom = format!(
        "{}{}{}",
        set.bottom_left,
        line(set.horizontal_bottom),
        set.bottom_right
    );
    [
        vec![Span::styled(top, theme.border)],
        vec![
            Span::styled(set.vertical_left.to_string(), theme.border),
            Span::styled(format!(" {key} "), theme.accent),
            Span::styled(set.vertical_right.to_string(), theme.border),
        ],
        vec![Span::styled(bottom, theme.border)],
    ]
}

/// The left part on three rows: the notice on the middle one, or the hints that fit in `room`,
/// each key in its box and its word after it on the middle row, blanks above and below.
fn boxed_left(
    bar: &Bar<'_>,
    theme: &Theme,
    room: usize,
    set: &border::Set<'_>,
) -> [Vec<Span<'static>>; 3] {
    if bar.notice.is_some() {
        return [Vec::new(), left_spans(bar, theme, room), Vec::new()];
    }
    let mut rows: [Vec<Span<'static>>; 3] = Default::default();
    for (i, h) in fit_hints(&bar.hints, true, room).iter().enumerate() {
        let word = format!(" {}", h.word);
        let blank = " ".repeat(width(&word));
        for (row, segment) in rows.iter_mut().zip(key_box(&h.key, set, theme)) {
            if i > 0 {
                row.push(Span::raw(BOXED_SEP));
            }
            row.extend(segment);
        }
        rows[0].push(Span::raw(blank.clone()));
        rows[1].push(Span::styled(word, theme.muted));
        rows[2].push(Span::raw(blank));
    }
    rows
}

impl MailScreen {
    pub(super) fn pane(&self) -> Pane {
        let prompting = self
            .compose
            .as_ref()
            .is_some_and(|c| c.captures_text() && !c.confirming_discard());
        match self.focus {
            Focus::Compose if prompting => Pane::Prompt,
            Focus::Compose => Pane::Compose,
            Focus::Sidebar => Pane::Sidebar,
            Focus::Reader => Pane::Reader,
            Focus::List if self.in_drafts() => Pane::Drafts,
            Focus::List => Pane::List,
        }
    }

    /// The bar from the screen's state: the focused pane's hints, the notice, the accounts.
    pub(super) fn bar(&self, theme: &Theme) -> Bar<'_> {
        let glyphs = Glyphs::of(self.ctx.icons);
        let provider = self.compose.as_ref().map_or("", |c| {
            self.kind_at(c.composing().account_index).provider_name()
        });
        let pane = self.pane();
        let mut hints = hints(pane, &self.ctx.keymap, self.sidebar_visible(), provider);
        // Range mode's lasting cue, once its notice has expired. First, because `fit_hints`
        // keeps whole hints from the left and trims the rest, so an appended entry would be
        // drawn at neither snapshot size. Only in the list: `list.clear_selection` is bound
        // there alone, so the hint would name a key that does something else in another pane.
        if pane == Pane::List && self.bulk.range_on() {
            hints.insert(
                0,
                hint(self.ctx.keymap.label(Action::ListClearSelection), "range"),
            );
        }
        let right = self
            .accounts
            .iter()
            .enumerate()
            .map(|(i, account)| {
                let status = self.status.status_of(account);
                let text = account_part(&escape(&account.label), status, &glyphs);
                (self.account_color(i, theme), text)
            })
            .collect();
        let discarding = self
            .compose
            .as_ref()
            .is_some_and(Composer::confirming_discard);
        let notice = if discarding {
            Some((DISCARD_QUESTION, true))
        } else {
            self.status
                .notice()
                .map(|t| (t, self.status.notice_is_error()))
        };
        Bar {
            hints,
            show_hints: self.hints,
            notice,
            dot: glyphs.account,
            right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_util::{keymap, render_lines};

    fn words(hints: &[Hint]) -> Vec<String> {
        hints
            .iter()
            .map(|h| format!("{} {}", h.key, h.word))
            .collect()
    }

    #[test]
    fn each_pane_has_its_hints_in_order() {
        let km = keymap();
        assert_eq!(
            words(&hints(Pane::List, &km, true, "Gmail")),
            [
                "Enter open",
                "j/k move",
                "Tab sidebar",
                "r reply",
                "c compose",
                "e archive",
                "d trash",
                "/ filter",
                "? help"
            ]
        );
        assert_eq!(
            words(&hints(Pane::Sidebar, &km, true, "Gmail")),
            ["Enter open", "Space fold", "j/k move", "Tab list", "? help"]
        );
        assert_eq!(
            words(&hints(Pane::Reader, &km, true, "Gmail")),
            [
                "n/p message",
                "Tab fold",
                "H headers",
                "r reply",
                "e archive",
                "q back",
                "? help"
            ]
        );
        assert_eq!(
            words(&hints(Pane::Drafts, &km, true, "Gmail")),
            ["Enter open", "d discard", "Tab sidebar", "? help"]
        );
        assert_eq!(
            words(&hints(Pane::Compose, &km, true, "Microsoft 365")),
            [
                "y send",
                "s save to Microsoft 365",
                "e edit",
                "t headers",
                "a attach",
                "x remove",
                "q keep",
                "d discard",
                "? help"
            ]
        );
    }

    #[test]
    fn a_prompt_shows_its_own_keys_and_no_help() {
        assert_eq!(
            words(&hints(Pane::Prompt, &keymap(), true, "Gmail")),
            ["Enter accept", "Tab complete", "Esc cancel"]
        );
    }

    #[test]
    fn a_hidden_sidebar_drops_its_hint_and_a_remapped_key_shows_its_label() {
        let km = keymap();
        assert!(
            !words(&hints(Pane::List, &km, false, "Gmail")).contains(&"Tab sidebar".to_string())
        );
        assert!(
            !words(&hints(Pane::Drafts, &km, false, "Gmail")).contains(&"Tab sidebar".to_string())
        );
        let text = "[keys]\n\"list.archive\" = \"A\"\n";
        let path = std::path::Path::new("/c/config.toml");
        let config = rail_core::config::parse(text, path).unwrap();
        let km =
            crate::tui::keys::Keymap::from_config(&config.keys, config.general.keymap, text, path)
                .unwrap();
        assert!(words(&hints(Pane::List, &km, true, "Gmail")).contains(&"A archive".to_string()));
    }

    #[test]
    fn trimming_keeps_whole_hints_and_help_last() {
        let km = keymap();
        let all = hints(Pane::List, &km, true, "Gmail");
        let full = hint_width(&all, false);
        assert_eq!(fit_hints(&all, false, full).len(), all.len());
        let fewer = fit_hints(&all, false, full - 1);
        assert_eq!(
            fewer.len(),
            all.len() - 1,
            "one hint drops, not a character"
        );
        assert_eq!(fewer.last().unwrap().word, "help");
        assert_eq!(
            fewer[fewer.len() - 2].word,
            "trash",
            "the one before help drops"
        );
        let tiny = fit_hints(&all, false, 20);
        assert_eq!(words(&tiny), ["Enter open", "? help"]);
        let only_help = fit_hints(&all, false, 6);
        assert_eq!(words(&only_help), ["? help"]);
    }

    fn draw_bar(bar: &Bar<'_>, cols: u16) -> String {
        let theme = Theme::default();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(cols, 1)).unwrap();
        terminal
            .draw(|frame| render_hints(frame, frame.area(), &theme, bar, Icons::Unicode))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..cols)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect()
    }

    fn sample_bar(notice: Option<(&'static str, bool)>) -> Bar<'static> {
        Bar {
            hints: hints(Pane::List, &keymap(), true, "Gmail"),
            show_hints: true,
            notice,
            dot: "●",
            right: vec![
                (Color::Green, "work ✓".to_string()),
                (Color::Red, "home ⟳".to_string()),
            ],
        }
    }

    #[test]
    fn the_bar_draws_hints_left_and_accounts_right() {
        let row = draw_bar(&sample_bar(None), 80);
        assert!(row.starts_with("Enter open  j/k move"), "{row}");
        assert!(row.ends_with("● work ✓ · ● home ⟳"), "{row}");
        assert!(row.contains("? help"), "{row}");
        assert_eq!(width(&row), 80);
    }

    #[test]
    fn a_notice_replaces_the_hints_and_a_narrow_bar_keeps_help() {
        let row = draw_bar(&sample_bar(Some(("Archived", false))), 80);
        assert!(row.starts_with("Archived"), "{row}");
        assert!(!row.contains("Enter open"), "{row}");
        let narrow = draw_bar(&sample_bar(None), 30);
        assert!(narrow.contains("? help"), "{narrow}");
        assert_eq!(width(&narrow), 30);
    }

    fn bar_buffer(bar: &Bar<'_>, cols: u16) -> ratatui::buffer::Buffer {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(cols, 1)).unwrap();
        terminal
            .draw(|frame| render_hints(frame, frame.area(), &Theme::default(), bar, Icons::Unicode))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn accounts_too_wide_for_the_room_are_cut_as_one_muted_string() {
        let bar = sample_bar(None);
        let row = draw_bar(&bar, 20);
        assert!(row.starts_with("? help"), "help keeps its room: {row}");
        assert!(row.contains("● work"), "{row}");
        assert!(!row.contains("home"), "the right part is cut: {row}");
        assert_eq!(width(&row), 20);
        let buffer = bar_buffer(&bar, 20);
        let dot = (0..20u16)
            .find(|x| buffer[(*x, 0)].symbol() == "●")
            .unwrap();
        assert_eq!(
            buffer[(dot, 0)].fg,
            Theme::default().muted.fg.unwrap(),
            "a cut part loses the account colours"
        );
    }

    #[test]
    fn an_error_notice_is_drawn_in_the_error_style() {
        let theme = Theme::default();
        let error = bar_buffer(&sample_bar(Some(("Could not archive", true))), 80);
        assert_eq!(error[(0, 0)].fg, theme.error.fg.unwrap());
        let plain = bar_buffer(&sample_bar(Some(("Archived", false))), 80);
        assert_eq!(plain[(0, 0)].fg, theme.muted.fg.unwrap());
    }

    #[test]
    fn account_names_in_the_bar_are_escaped() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = crate::tui::test_util::mail_ctx(
            &rail_core::paths::Paths::all_in(tmp.path()),
            std::sync::Arc::new(crate::tui::data::FakeDaemon::default()),
        );
        let mut accounts = crate::tui::test_util::work_and_home();
        accounts[0].label = "wo\u{1b}[2Jrk".into();
        let screen = MailScreen::new(ctx, accounts);
        let bar = screen.bar(&Theme::default());
        let text = &bar.right[0].1;
        assert!(!text.contains('\u{1b}'), "{text:?}");
        assert!(text.contains("wo\\u{1b}[2Jrk"), "{text:?}");
    }

    #[test]
    fn boxed_trimming_counts_the_box_around_each_key() {
        let all = hints(Pane::List, &keymap(), true, "Gmail");
        let help = &all[all.len() - 1..];
        assert_eq!(
            hint_width(help, true),
            10,
            "`? help` boxed is 1 + 4 + 1 + 4 cells"
        );
        assert_eq!(hint_width(help, false), 6);
        let full = hint_width(&all, true);
        assert_eq!(fit_hints(&all, true, full).len(), all.len());
        assert_eq!(fit_hints(&all, true, full - 1).len(), all.len() - 1);
        // `Enter open` boxed is 9 + 1 + 4 = 14; with one space and `? help`: 14 + 1 + 10 = 25.
        assert_eq!(words(&fit_hints(&all, true, 25)), ["Enter open", "? help"]);
        assert_eq!(words(&fit_hints(&all, true, 24)), ["? help"]);
        assert_eq!(
            words(&fit_hints(&all, false, 28)),
            ["Enter open", "j/k move", "? help"],
            "the one-row bar counts no boxes: 10 + 2 + 8 + 2 + 6"
        );
        assert_eq!(words(&fit_hints(&all, true, 28)), ["Enter open", "? help"]);
    }

    fn two_hints() -> Bar<'static> {
        Bar {
            hints: vec![
                hint("Tab".to_string(), "pane"),
                hint("Enter".to_string(), "open"),
            ],
            show_hints: true,
            notice: None,
            dot: "●",
            right: Vec::new(),
        }
    }

    fn boxed_rows(bar: &Bar<'_>, cols: u16, icons: Icons) -> Vec<String> {
        render_lines(cols, 3, |frame| {
            render_hints(frame, frame.area(), &Theme::default(), bar, icons);
        })
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
    }

    #[test]
    fn three_rows_draw_each_key_in_its_own_box() {
        assert_eq!(
            boxed_rows(&two_hints(), 40, Icons::Unicode),
            [
                "┌─────┐      ┌───────┐",
                "│ Tab │ pane │ Enter │ open",
                "└─────┘      └───────┘",
            ]
        );
        assert_eq!(
            boxed_rows(&two_hints(), 40, Icons::Ascii),
            [
                "+-----+      +-------+",
                "| Tab | pane | Enter | open",
                "+-----+      +-------+",
            ]
        );
    }

    #[test]
    fn boxed_keys_take_the_accent_and_their_lines_the_border_style() {
        let theme = Theme::default();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| render_hints(frame, frame.area(), &theme, &two_hints(), Icons::Unicode))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        assert_eq!(buffer[(0, 0)].fg, theme.border.fg.unwrap());
        assert_eq!(buffer[(0, 1)].fg, theme.border.fg.unwrap());
        assert_eq!(buffer[(2, 1)].symbol(), "T");
        assert_eq!(buffer[(2, 1)].fg, theme.accent.fg.unwrap());
        assert!(
            buffer[(2, 1)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(buffer[(8, 1)].symbol(), "p");
        assert_eq!(buffer[(8, 1)].fg, theme.muted.fg.unwrap());
    }

    #[test]
    fn boxed_bar_keeps_the_accounts_and_a_notice_on_the_middle_row() {
        let rows = boxed_rows(&sample_bar(None), 120, Icons::Unicode);
        assert!(rows[0].starts_with("┌───────┐      ┌─────┐"), "{}", rows[0]);
        assert!(
            rows[1].starts_with("│ Enter │ open │ j/k │ move"),
            "{}",
            rows[1]
        );
        assert!(rows[1].ends_with("● work ✓ · ● home ⟳"), "{}", rows[1]);
        assert_eq!(width(&rows[1]), 120, "the accounts sit at the right edge");
        assert!(rows[1].contains("│ ? │ help"), "{}", rows[1]);
        assert!(!rows[0].contains('●') && !rows[2].contains('●'));
        let notice = boxed_rows(&sample_bar(Some(("Archived", false))), 80, Icons::Unicode);
        assert_eq!(notice[0], "", "no boxes above a notice");
        assert!(notice[1].starts_with("Archived"), "{}", notice[1]);
        assert!(notice[1].ends_with("● work ✓ · ● home ⟳"), "{}", notice[1]);
        assert_eq!(notice[2], "");
    }
}
