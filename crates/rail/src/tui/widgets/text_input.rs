//! A single-line text input with a cursor and a masked mode for secrets, and the labelled
//! field that draws it.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::FieldKeys;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::theme::Theme;

/// Columns reserved for the label in front of every field.
pub const LABEL_WIDTH: u16 = 15;
/// Rows one field takes: the value, then its error or hint.
pub const FIELD_HEIGHT: u16 = 2;

/// Editable single-line text. The value may be a secret, so `Debug` never prints it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    /// In characters, not bytes.
    cursor: usize,
    masked: bool,
    /// `general.field_keys`, which the screen hands over before each key.
    keys: FieldKeys,
}

impl fmt::Debug for TextInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextInput")
            .field("len", &self.len())
            .field("cursor", &self.cursor)
            .field("masked", &self.masked)
            .finish()
    }
}

impl TextInput {
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor position in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Replace the text (when the wizard changed it) and put the cursor at the end.
    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
        self.cursor = self.len();
    }

    /// Show one dot per character instead of the text.
    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    /// Edit with `keys` from now on: the screen calls this before each key, so a change to
    /// `general.field_keys` reaches fields that are already open.
    pub fn set_keys(&mut self, keys: FieldKeys) {
        self.keys = keys;
    }

    /// Apply an editing key. Returns true when the text changed.
    ///
    /// With `basic` keys, a key with Ctrl or Alt does nothing. With `readline`, the keys with
    /// Ctrl or Alt edit as bash does. Ctrl or Alt with an arrow moves by word,
    /// with `Backspace` or `Delete` removes a word, and a word is letters and digits. In a
    /// masked field the whole value is one word, so a word step never shows where a secret's
    /// symbols are.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.keys == FieldKeys::Basic && (ctrl || alt) {
            return false;
        }
        match key.code {
            KeyCode::Char(c) if ctrl && !alt => self.control(c),
            KeyCode::Char(c) if alt && !ctrl => self.meta(c),
            KeyCode::Backspace if ctrl || alt => self.remove(self.word_start(), self.cursor),
            KeyCode::Delete if ctrl || alt => self.remove(self.cursor, self.word_end()),
            KeyCode::Left if ctrl || alt => self.move_to(self.word_start()),
            KeyCode::Right if ctrl || alt => self.move_to(self.word_end()),
            _ if ctrl || alt => false,
            KeyCode::Char(c) => {
                self.insert(c);
                true
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.back(),
            KeyCode::Right => self.forward(),
            KeyCode::Home => self.move_to(0),
            KeyCode::End => self.move_to(self.len()),
            _ => false,
        }
    }

    /// Ctrl with a letter, as bash reads it. `Ctrl-h` is `Backspace`, as some terminals send
    /// it for that key, even though others send it for `Ctrl-Backspace`.
    fn control(&mut self, c: char) -> bool {
        match c {
            'a' => self.move_to(0),
            'e' => self.move_to(self.len()),
            'u' => self.remove(0, self.cursor),
            'k' => self.remove(self.cursor, self.len()),
            'w' => self.remove(self.unspaced_start(), self.cursor),
            'h' => self.backspace(),
            'b' => self.back(),
            'f' => self.forward(),
            'd' => self.delete(),
            _ => false,
        }
    }

    /// Alt with a letter, as bash reads it: also what macOS terminals send for Option with an
    /// arrow or `Delete`.
    fn meta(&mut self, c: char) -> bool {
        match c {
            'b' => self.move_to(self.word_start()),
            'f' => self.move_to(self.word_end()),
            'd' => self.remove(self.cursor, self.word_end()),
            _ => false,
        }
    }

    /// What the screen shows: the text, or one dot per character when masked.
    pub fn display(&self) -> String {
        if self.masked {
            "•".repeat(self.len())
        } else {
            self.value.clone()
        }
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(i, _)| i)
    }

    /// Cursor moves never change the text, so they report `false`.
    fn move_to(&mut self, cursor: usize) -> bool {
        self.cursor = cursor.min(self.len());
        false
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_index(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        self.remove(self.cursor.saturating_sub(1), self.cursor)
    }

    fn delete(&mut self) -> bool {
        self.remove(self.cursor, (self.cursor + 1).min(self.len()))
    }

    fn back(&mut self) -> bool {
        self.move_to(self.cursor.saturating_sub(1))
    }

    fn forward(&mut self) -> bool {
        self.move_to(self.cursor + 1)
    }

    /// Remove the characters from `start` to `end` and leave the cursor at `start`.
    fn remove(&mut self, start: usize, end: usize) -> bool {
        if start >= end {
            return false;
        }
        let bytes = self.byte_index(start)..self.byte_index(end);
        self.value.replace_range(bytes, "");
        self.cursor = start;
        true
    }

    /// Where the word before the cursor starts, a word being letters and digits.
    fn word_start(&self) -> usize {
        let in_word = self.words(char::is_alphanumeric);
        self.cursor - word_len(self.before().chars().rev(), in_word)
    }

    /// Where the word after the cursor ends, a word being letters and digits.
    fn word_end(&self) -> usize {
        let after = &self.value[self.byte_index(self.cursor)..];
        self.cursor + word_len(after.chars(), self.words(char::is_alphanumeric))
    }

    /// Where the word before the cursor starts, a word being anything but spaces.
    fn unspaced_start(&self) -> usize {
        let in_word = self.words(|c| !c.is_whitespace());
        self.cursor - word_len(self.before().chars().rev(), in_word)
    }

    /// The characters words are made of: `in_word`'s, or every one in a masked field.
    fn words(&self, in_word: fn(char) -> bool) -> fn(char) -> bool {
        if self.masked { |_| true } else { in_word }
    }

    fn before(&self) -> &str {
        &self.value[..self.byte_index(self.cursor)]
    }
}

/// How many characters a word step crosses, reading away from the cursor: whatever is not
/// `in_word` up to the next word, then that word. So in `me@example.com` a step back from
/// the end stops before `com`, then `example`, then `me` when only letters and digits make a
/// word, and goes to the start when anything but spaces does.
fn word_len(chars: impl Iterator<Item = char>, in_word: fn(char) -> bool) -> usize {
    let mut seen = false;
    chars
        .take_while(|&c| {
            let inside = in_word(c);
            seen |= inside;
            inside || !seen
        })
        .count()
}

/// A labelled input: `label  value` on the first row, the error (or else the hint) below.
pub struct TextField<'a> {
    pub label: &'a str,
    pub input: &'a TextInput,
    pub focused: bool,
    pub error: Option<&'a str>,
    pub hint: Option<&'a str>,
    pub theme: &'a Theme,
}

impl TextField<'_> {
    /// Where the terminal cursor goes when this field has focus.
    pub fn cursor_position(&self, area: Rect) -> Position {
        let (start, _) = self.window(area.width);
        let column = u16::try_from(self.input.cursor() - start).unwrap_or(u16::MAX);
        Position::new(
            area.x.saturating_add(LABEL_WIDTH).saturating_add(column),
            area.y,
        )
    }

    /// The slice of the value that is shown: its first character index and the columns
    /// available after the label. The window follows the cursor so it is always visible.
    /// Both `render` and `cursor_position` use this, so they cannot disagree.
    fn window(&self, width: u16) -> (usize, usize) {
        let room = usize::from(width.saturating_sub(LABEL_WIDTH));
        let start = self.input.cursor().saturating_sub(room.saturating_sub(1));
        (start, room)
    }
}

impl Widget for TextField<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let label_style = if self.focused {
            self.theme.accent
        } else {
            self.theme.text
        };
        let width = usize::from(LABEL_WIDTH);
        let (start, room) = self.window(area.width);
        let shown: String = self
            .input
            .display()
            .chars()
            .skip(start)
            .take(room)
            .collect();
        let value = Line::from(vec![
            Span::styled(format!("{:<width$}", self.label), label_style),
            Span::styled(shown, self.theme.text),
        ]);
        let note = match (self.error, self.hint) {
            (Some(error), _) => Line::styled(format!("{:width$}{error}", ""), self.theme.error),
            (None, Some(hint)) => Line::styled(format!("{:width$}{hint}", ""), self.theme.muted),
            (None, None) => Line::default(),
        };
        Paragraph::new(vec![value, note]).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_util::{SIZES, ctrl, key, render_lines};

    fn typed(text: &str) -> TextInput {
        let mut input = TextInput::default();
        for c in text.chars() {
            assert!(input.handle_key(key(KeyCode::Char(c))));
        }
        input
    }

    #[test]
    fn typing_inserts_and_backspace_deletes() {
        let mut input = typed("me@x.io");
        assert_eq!(input.value(), "me@x.io");
        assert!(input.handle_key(key(KeyCode::Backspace)));
        assert_eq!(input.value(), "me@x.i");
        assert_eq!(input.cursor(), 6);
    }

    #[test]
    fn cursor_moves_and_inserts_in_the_middle() {
        let mut input = typed("ac");
        assert!(!input.handle_key(key(KeyCode::Left)));
        assert!(input.handle_key(key(KeyCode::Char('b'))));
        assert_eq!(input.value(), "abc");
        assert!(!input.handle_key(key(KeyCode::Home)));
        assert_eq!(input.cursor(), 0);
        assert!(
            !input.handle_key(key(KeyCode::Backspace)),
            "nothing before the cursor"
        );
        assert!(input.handle_key(key(KeyCode::Delete)));
        assert_eq!(input.value(), "bc");
        assert!(!input.handle_key(key(KeyCode::End)));
        assert_eq!(input.cursor(), 2);
        assert!(!input.handle_key(key(KeyCode::Right)), "already at the end");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn editing_counts_characters_not_bytes() {
        let mut input = typed("né");
        assert_eq!(input.cursor(), 2);
        assert!(input.handle_key(key(KeyCode::Backspace)));
        assert_eq!(input.value(), "n");
    }

    #[test]
    fn control_keys_do_not_edit() {
        let mut input = typed("ab");
        assert!(!input.handle_key(ctrl('x')));
        assert!(!input.handle_key(key(KeyCode::Enter)));
        assert_eq!(input.value(), "ab");
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn alt(c: char) -> KeyEvent {
        with(KeyCode::Char(c), KeyModifiers::ALT)
    }

    #[test]
    fn ctrl_w_removes_back_to_a_space_as_bash_does() {
        let mut input = typed("to me@example.com  ");
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "to ");
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "");
        assert!(!input.handle_key(ctrl('w')), "nothing before the cursor");

        let mut input = typed("from:alice bob");
        input.handle_key(key(KeyCode::Left));
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "from:alice b");
        assert_eq!(input.cursor(), 11);
    }

    #[test]
    fn word_deletes_back_take_letters_and_digits_as_bash_does() {
        let keys = [
            with(KeyCode::Backspace, KeyModifiers::CONTROL),
            with(KeyCode::Backspace, KeyModifiers::ALT),
        ];
        for pressed in keys {
            let mut input = typed("to foo_bar me@example.com");
            for left in [
                "to foo_bar me@example.",
                "to foo_bar me@",
                "to foo_bar ",
                "to foo_",
                "to ",
                "",
            ] {
                assert!(input.handle_key(pressed), "{pressed:?}");
                assert_eq!(input.value(), left, "{pressed:?}");
            }
            assert!(!input.handle_key(pressed), "nothing before the cursor");
        }
    }

    #[test]
    fn word_deletes_forward_keep_the_cursor() {
        for pressed in [with(KeyCode::Delete, KeyModifiers::CONTROL), alt('d')] {
            let mut input = typed("from:alice bob");
            input.handle_key(key(KeyCode::Home));
            for left in [":alice bob", " bob", ""] {
                assert!(input.handle_key(pressed), "{pressed:?}");
                assert_eq!(input.value(), left, "{pressed:?}");
                assert_eq!(input.cursor(), 0, "{pressed:?}");
            }
            assert!(!input.handle_key(pressed), "nothing after the cursor");
        }
    }

    #[test]
    fn line_keys_move_and_cut_as_bash_does() {
        let mut input = typed("from:alice bob");
        assert!(!input.handle_key(ctrl('a')));
        assert_eq!(input.cursor(), 0);
        assert!(!input.handle_key(ctrl('e')));
        assert_eq!(input.cursor(), 14);

        input.handle_key(ctrl('a'));
        for _ in 0..5 {
            input.handle_key(key(KeyCode::Right));
        }
        assert!(input.handle_key(ctrl('u')));
        assert_eq!(input.value(), "alice bob");
        assert_eq!(input.cursor(), 0);
        assert!(!input.handle_key(ctrl('u')), "nothing before the cursor");

        input.handle_key(key(KeyCode::Right));
        assert!(input.handle_key(ctrl('k')));
        assert_eq!(input.value(), "a");
        assert_eq!(input.cursor(), 1);
        assert!(!input.handle_key(ctrl('k')), "nothing after the cursor");
    }

    #[test]
    fn word_moves_change_nothing_but_the_cursor() {
        let backs = [
            with(KeyCode::Left, KeyModifiers::CONTROL),
            with(KeyCode::Left, KeyModifiers::ALT),
            alt('b'),
        ];
        let forwards = [
            with(KeyCode::Right, KeyModifiers::CONTROL),
            with(KeyCode::Right, KeyModifiers::ALT),
            alt('f'),
        ];
        for (back, forward) in backs.into_iter().zip(forwards) {
            let mut input = typed("re: the plan");
            for at in [8, 4, 0, 0] {
                assert!(!input.handle_key(back), "{back:?}");
                assert_eq!(input.cursor(), at, "{back:?}");
            }
            for at in [2, 7, 12, 12] {
                assert!(!input.handle_key(forward), "{forward:?}");
                assert_eq!(input.cursor(), at, "{forward:?}");
            }
            assert_eq!(input.value(), "re: the plan");
        }
    }

    #[test]
    fn word_editing_counts_characters_not_bytes() {
        let mut input = typed("café über");
        assert!(!input.handle_key(with(KeyCode::Left, KeyModifiers::CONTROL)));
        assert_eq!(input.cursor(), 5);
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "über");
        assert_eq!(input.cursor(), 0);
        input.handle_key(ctrl('e'));
        assert!(input.handle_key(with(KeyCode::Backspace, KeyModifiers::ALT)));
        assert_eq!(input.value(), "");
    }

    #[test]
    fn ctrl_h_removes_one_character_as_backspace_does() {
        let mut input = typed("me@x.io");
        assert!(input.handle_key(ctrl('h')));
        assert_eq!(input.value(), "me@x.i");
        assert_eq!(input.cursor(), 6);
        input.handle_key(key(KeyCode::Home));
        assert!(!input.handle_key(ctrl('h')), "nothing before the cursor");
    }

    #[test]
    fn ctrl_b_ctrl_f_and_ctrl_d_work_a_character_at_a_time_as_bash_does() {
        let mut input = typed("abc");
        assert!(!input.handle_key(ctrl('b')));
        assert!(!input.handle_key(ctrl('b')));
        assert_eq!(input.cursor(), 1);
        assert!(input.handle_key(ctrl('d')));
        assert_eq!(input.value(), "ac");
        assert_eq!(input.cursor(), 1);
        assert!(!input.handle_key(ctrl('f')));
        assert_eq!(input.cursor(), 2);
        assert!(!input.handle_key(ctrl('f')), "already at the end");
        assert_eq!(input.cursor(), 2);
        assert!(!input.handle_key(ctrl('d')), "nothing after the cursor");
        input.handle_key(key(KeyCode::Home));
        assert!(!input.handle_key(ctrl('b')), "already at the start");
        assert_eq!(input.cursor(), 0);
    }

    /// `basic` is Rail before the readline keys: a key with Ctrl or Alt changes nothing.
    #[test]
    fn basic_keys_leave_ctrl_and_alt_alone() {
        let mut input = typed("to me@example.com");
        input.set_keys(FieldKeys::Basic);
        let keys = [
            ctrl('a'),
            ctrl('w'),
            ctrl('u'),
            ctrl('h'),
            ctrl('b'),
            ctrl('d'),
            alt('b'),
            alt('d'),
            with(KeyCode::Backspace, KeyModifiers::ALT),
            with(KeyCode::Left, KeyModifiers::CONTROL),
        ];
        for pressed in keys {
            assert!(!input.handle_key(pressed), "{pressed:?}");
            assert_eq!(input.value(), "to me@example.com", "{pressed:?}");
            assert_eq!(input.cursor(), 17, "{pressed:?}");
        }
        assert!(input.handle_key(key(KeyCode::Backspace)));
        assert!(!input.handle_key(key(KeyCode::Home)));
        assert!(input.handle_key(key(KeyCode::Delete)));
        assert_eq!(input.value(), "o me@example.co");

        input.set_keys(FieldKeys::Readline);
        assert!(input.handle_key(ctrl('k')), "back to readline");
        assert_eq!(input.value(), "");
    }

    #[test]
    fn masked_word_keys_take_the_whole_value_as_one_word() {
        let mut input = typed("GOCSPX-ab cd");
        input.set_masked(true);
        assert!(!input.handle_key(with(KeyCode::Left, KeyModifiers::CONTROL)));
        assert_eq!(input.cursor(), 0, "stopped inside the secret");
        assert!(!input.handle_key(alt('f')));
        assert_eq!(input.cursor(), 12, "stopped inside the secret");

        input.handle_key(key(KeyCode::Left));
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "d");
        input.set_value("GOCSPX-ab cd");
        assert!(input.handle_key(with(KeyCode::Backspace, KeyModifiers::CONTROL)));
        assert_eq!(input.value(), "");
        input.set_value("GOCSPX-ab cd");
        input.handle_key(key(KeyCode::Home));
        assert!(input.handle_key(with(KeyCode::Delete, KeyModifiers::CONTROL)));
        assert_eq!(input.value(), "");
    }

    #[test]
    fn ctrl_alt_letters_do_not_edit() {
        let mut input = typed("ab");
        let both = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert!(!input.handle_key(with(KeyCode::Char('w'), both)));
        assert!(!input.handle_key(with(KeyCode::Char('d'), both)));
        assert!(!input.handle_key(alt('x')));
        assert_eq!(input.value(), "ab");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn set_value_puts_the_cursor_at_the_end() {
        let mut input = TextInput::default();
        input.set_value("work");
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn masked_mode_shows_one_dot_per_character() {
        let mut input = typed("hunter2");
        input.set_masked(true);
        assert_eq!(input.display(), "•••••••");
        assert_eq!(input.value(), "hunter2");
    }

    #[test]
    fn debug_never_prints_the_value() {
        let mut input = typed("hunter2");
        input.set_masked(true);
        let shown = format!("{input:?}");
        assert!(!shown.contains("hunter2"), "debug output leaked the value");
        assert!(shown.contains('7'), "{shown}");
    }

    #[test]
    fn field_renders_label_value_and_error_at_both_sizes() {
        let theme = Theme::default();
        let input = typed("me@example");
        for (width, height) in SIZES {
            let lines = render_lines(width, height, |frame| {
                let field = TextField {
                    label: "Email address",
                    input: &input,
                    focused: true,
                    error: Some("not an email address"),
                    hint: Some("never shown while there is an error"),
                    theme: &theme,
                };
                frame.render_widget(field, frame.area());
            });
            assert!(
                lines[0].starts_with("Email address  me@example"),
                "{lines:?}"
            );
            assert!(lines[1].contains("not an email address"), "{lines:?}");
            assert!(!lines[1].contains("never shown"), "{lines:?}");
        }
    }

    #[test]
    fn hint_renders_when_there_is_no_error() {
        let theme = Theme::default();
        let input = TextInput::default();
        let lines = render_lines(80, 24, |frame| {
            let field = TextField {
                label: "Colour",
                input: &input,
                focused: false,
                error: None,
                hint: Some("optional, for example blue"),
                theme: &theme,
            };
            frame.render_widget(field, frame.area());
        });
        assert!(lines[1].contains("optional, for example blue"), "{lines:?}");
    }

    #[test]
    fn masked_field_never_renders_the_secret() {
        let theme = Theme::default();
        let mut input = typed("hunter2");
        input.set_masked(true);
        let lines = render_lines(80, 24, |frame| {
            let field = TextField {
                label: "Client secret",
                input: &input,
                focused: true,
                error: None,
                hint: None,
                theme: &theme,
            };
            frame.render_widget(field, frame.area());
        });
        let text = lines.join("\n");
        assert!(text.contains("•••••••"), "masked dots missing");
        assert!(!text.contains("hunter2"), "the secret was rendered");
    }

    #[test]
    fn cursor_position_follows_the_cursor() {
        let theme = Theme::default();
        let input = typed("abc");
        let field = TextField {
            label: "Name",
            input: &input,
            focused: true,
            error: None,
            hint: None,
            theme: &theme,
        };
        let area = Rect::new(2, 5, 60, FIELD_HEIGHT);
        assert_eq!(
            field.cursor_position(area),
            Position::new(2 + LABEL_WIDTH + 3, 5)
        );
    }

    fn long_input() -> TextInput {
        let long: String = (0..90u8).map(|i| char::from(b'a' + i % 26)).collect();
        typed(&format!("{long}END"))
    }

    fn render_field(input: &TextInput) -> Vec<String> {
        let theme = Theme::default();
        render_lines(80, 24, |frame| {
            let field = TextField {
                label: "Name",
                input,
                focused: true,
                error: None,
                hint: None,
                theme: &theme,
            };
            frame.render_widget(field, frame.area());
        })
    }

    fn cursor_of(input: &TextInput) -> Position {
        let theme = Theme::default();
        let field = TextField {
            label: "Name",
            input,
            focused: true,
            error: None,
            hint: None,
            theme: &theme,
        };
        field.cursor_position(Rect::new(0, 0, 80, FIELD_HEIGHT))
    }

    #[test]
    fn long_value_shows_its_tail_with_the_cursor_on_the_last_column() {
        let mut input = long_input();
        let lines = render_field(&input);
        assert!(lines[0].trim_end().ends_with("END"), "tail not shown");
        assert_eq!(cursor_of(&input), Position::new(79, 0));

        input.handle_key(key(KeyCode::Home));
        let lines = render_field(&input);
        assert!(
            lines[0].starts_with("Name           abcdef"),
            "start not shown"
        );
        assert!(!lines[0].contains("END"), "tail still shown after Home");
        assert_eq!(cursor_of(&input), Position::new(15, 0));
    }

    #[test]
    fn masked_long_value_shows_only_dots() {
        let mut input = long_input();
        input.set_masked(true);
        let lines = render_field(&input);
        let shown = lines[0].trim_start_matches("Name           ").trim_end();
        assert_eq!(shown.chars().count(), 64, "cursor sits in the 65th column");
        assert!(
            shown.chars().all(|c| c == '•'),
            "non-dot character rendered"
        );
        assert_eq!(cursor_of(&input), Position::new(79, 0));
    }
}
