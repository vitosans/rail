//! `field_keys = "vi"`: a field edits as a shell's `set -o vi` line does. It opens in insert
//! mode, where keys type; `Esc` goes to normal mode, where they move and edit. In normal mode
//! the field leaves `Esc` to its screen, so a second `Esc` closes the prompt as before.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::TextInput;

/// Where a vi field is: typing, or reading commands, maybe halfway through one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Vi {
    #[default]
    Insert,
    Normal,
    /// `d` or `c` was pressed and waits for its motion.
    Operator(Operator),
    /// `r` was pressed and waits for the character.
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operator {
    Delete,
    Change,
}

/// What a motion key means: a place in the line, and whether `d` takes the character there
/// too (`e`, `$`), as vi's inclusive motions do.
struct Motion {
    to: usize,
    inclusive: bool,
}

/// What a character is for vi's word motions. `w`, `b` and `e` stop where the class changes
/// (letters, digits and `_` against other symbols); `W`, `B` and `E` only at spaces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Blank,
    Word,
    Symbol,
}

impl TextInput {
    /// True while `Esc` is the field's: in insert mode it goes to normal mode. Screens check
    /// this before treating `Esc` as close or back.
    pub fn takes_esc(&self) -> bool {
        self.keys == rail_core::config::FieldKeys::Vi && self.vi != Vi::Normal
    }

    pub(super) fn vi_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match self.vi {
            Vi::Insert => self.vi_insert(key, ctrl, alt),
            _ if ctrl || alt => false,
            Vi::Normal => self.vi_normal(key.code),
            Vi::Operator(op) => self.vi_operator(op, key.code),
            Vi::Replace => self.vi_replace(key.code),
        }
    }

    /// Insert mode types as the `basic` keys do, with bash's vi-insert `Ctrl-w`, `Ctrl-u` and
    /// `Ctrl-h`.
    fn vi_insert(&mut self, key: KeyEvent, ctrl: bool, alt: bool) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.vi = Vi::Normal;
                self.move_to(self.cursor.saturating_sub(1))
            }
            KeyCode::Char('w') if ctrl && !alt => self.remove(self.unspaced_start(), self.cursor),
            KeyCode::Char('u') if ctrl && !alt => self.remove(0, self.cursor),
            KeyCode::Char('h') if ctrl && !alt => self.backspace(),
            _ if ctrl || alt => false,
            _ => self.plain_key(key.code),
        }
    }

    fn vi_normal(&mut self, code: KeyCode) -> bool {
        if let Some(motion) = self.motion(code) {
            return self.normal_move(motion.to);
        }
        let end = self.len();
        match code {
            KeyCode::Char('i') => self.insert_at(self.cursor),
            KeyCode::Char('a') => self.insert_at((self.cursor + 1).min(end)),
            KeyCode::Char('I') => self.insert_at(self.first_non_blank()),
            KeyCode::Char('A') => self.insert_at(end),
            KeyCode::Char('x') | KeyCode::Delete => self.normal_edit(Self::delete),
            KeyCode::Char('X') => self.normal_edit(Self::backspace),
            KeyCode::Char('D') => self.normal_edit(|s| s.remove(s.cursor, s.len())),
            KeyCode::Char('C') => self.change(self.cursor, end),
            KeyCode::Char('s') => self.change(self.cursor, (self.cursor + 1).min(end)),
            KeyCode::Char('S') => self.change(0, end),
            KeyCode::Char('~') => self.toggle_case(),
            KeyCode::Char('d') => self.pending(Vi::Operator(Operator::Delete)),
            KeyCode::Char('c') => self.pending(Vi::Operator(Operator::Change)),
            KeyCode::Char('r') if end > 0 => self.pending(Vi::Replace),
            _ => false,
        }
    }

    /// `d` or `c` with a motion, or doubled (`dd`, `cc`) for the whole line. Anything else
    /// cancels it.
    fn vi_operator(&mut self, op: Operator, code: KeyCode) -> bool {
        self.vi = Vi::Normal;
        let doubled = matches!(
            (op, code),
            (Operator::Delete, KeyCode::Char('d')) | (Operator::Change, KeyCode::Char('c'))
        );
        let (start, end) = if doubled {
            (0, self.len())
        } else {
            // `cw` changes to the end of the word, as vi's does, not up to the next one.
            let code = match (op, code) {
                (Operator::Change, KeyCode::Char('w')) if !self.on_blank() => KeyCode::Char('e'),
                (Operator::Change, KeyCode::Char('W')) if !self.on_blank() => KeyCode::Char('E'),
                _ => code,
            };
            let Some(motion) = self.motion(code) else {
                return false;
            };
            let (from, to) = (self.cursor.min(motion.to), self.cursor.max(motion.to));
            let to = if motion.inclusive { to + 1 } else { to };
            (from, to.min(self.len()))
        };
        match op {
            Operator::Delete => self.normal_edit(|s| s.remove(start, end)),
            Operator::Change => self.change(start, end),
        }
    }

    fn vi_replace(&mut self, code: KeyCode) -> bool {
        self.vi = Vi::Normal;
        let KeyCode::Char(c) = code else {
            return false;
        };
        let at = self.cursor;
        if at >= self.len() {
            return false;
        }
        self.remove(at, at + 1);
        self.insert(c);
        self.cursor = at;
        true
    }

    /// The motion keys, the same in normal mode and after `d` or `c`.
    fn motion(&self, code: KeyCode) -> Option<Motion> {
        let exclusive = |to| {
            Some(Motion {
                to,
                inclusive: false,
            })
        };
        let inclusive = |to| {
            Some(Motion {
                to,
                inclusive: true,
            })
        };
        let at = self.cursor;
        match code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                exclusive(at.saturating_sub(1))
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                exclusive((at + 1).min(self.len()))
            }
            KeyCode::Char('0') | KeyCode::Home => exclusive(0),
            KeyCode::Char('^') => exclusive(self.first_non_blank()),
            KeyCode::Char('$') | KeyCode::End => inclusive(self.len().saturating_sub(1)),
            KeyCode::Char('w') => exclusive(self.next_word(false)),
            KeyCode::Char('W') => exclusive(self.next_word(true)),
            KeyCode::Char('b') => exclusive(self.previous_word(false)),
            KeyCode::Char('B') => exclusive(self.previous_word(true)),
            KeyCode::Char('e') => inclusive(self.word_end_here(false)),
            KeyCode::Char('E') => inclusive(self.word_end_here(true)),
            _ => None,
        }
    }

    /// In normal mode the cursor sits on a character, so it never passes the last one.
    fn normal_move(&mut self, to: usize) -> bool {
        self.move_to(to.min(self.len().saturating_sub(1)))
    }

    fn normal_edit(&mut self, edit: impl FnOnce(&mut Self) -> bool) -> bool {
        let changed = edit(self);
        self.normal_move(self.cursor);
        changed
    }

    fn insert_at(&mut self, at: usize) -> bool {
        self.vi = Vi::Insert;
        self.move_to(at)
    }

    /// Remove `start..end` and type in its place.
    fn change(&mut self, start: usize, end: usize) -> bool {
        let changed = self.remove(start, end);
        self.cursor = start;
        self.vi = Vi::Insert;
        changed
    }

    fn pending(&mut self, state: Vi) -> bool {
        self.vi = state;
        false
    }

    fn toggle_case(&mut self) -> bool {
        let at = self.cursor;
        let Some(c) = self.value.chars().nth(at) else {
            return false;
        };
        let flipped: String = if c.is_uppercase() {
            c.to_lowercase().collect()
        } else {
            c.to_uppercase().collect()
        };
        let changed = flipped != c.to_string();
        if changed {
            let bytes = self.byte_index(at)..self.byte_index(at + 1);
            self.value.replace_range(bytes, &flipped);
        }
        self.normal_move(at + flipped.chars().count());
        changed
    }

    fn chars(&self) -> Vec<char> {
        self.value.chars().collect()
    }

    /// A character's class; in a masked field every character is one word, so a motion never
    /// shows where a secret's symbols are.
    fn class(&self, c: char, big: bool) -> Class {
        if c.is_whitespace() && !self.masked {
            Class::Blank
        } else if big || self.masked || c.is_alphanumeric() || c == '_' {
            Class::Word
        } else {
            Class::Symbol
        }
    }

    fn on_blank(&self) -> bool {
        self.value
            .chars()
            .nth(self.cursor)
            .is_none_or(|c| self.class(c, false) == Class::Blank)
    }

    fn first_non_blank(&self) -> usize {
        self.chars()
            .iter()
            .position(|&c| self.class(c, false) != Class::Blank)
            .unwrap_or(0)
    }

    /// `w`: the start of the next word, or the end of the line.
    fn next_word(&self, big: bool) -> usize {
        let chars = self.chars();
        let class = |i: usize| self.class(chars[i], big);
        let mut i = self.cursor;
        if i >= chars.len() {
            return chars.len();
        }
        let start = class(i);
        while i < chars.len() && class(i) == start && start != Class::Blank {
            i += 1;
        }
        while i < chars.len() && class(i) == Class::Blank {
            i += 1;
        }
        i
    }

    /// `b`: the start of this word, or of the one before when already at a start.
    fn previous_word(&self, big: bool) -> usize {
        let chars = self.chars();
        let class = |i: usize| self.class(chars[i], big);
        let mut i = self.cursor.min(chars.len());
        while i > 0 && class(i - 1) == Class::Blank {
            i -= 1;
        }
        if i == 0 {
            return 0;
        }
        let word = class(i - 1);
        while i > 0 && class(i - 1) == word {
            i -= 1;
        }
        i
    }

    /// `e`: the last character of this word, or of the next when already at an end.
    fn word_end_here(&self, big: bool) -> usize {
        let chars = self.chars();
        let class = |i: usize| self.class(chars[i], big);
        let last = chars.len().saturating_sub(1);
        let mut i = self.cursor + 1;
        while i < chars.len() && class(i) == Class::Blank {
            i += 1;
        }
        if i >= chars.len() {
            return last;
        }
        let word = class(i);
        while i < last && class(i + 1) == word {
            i += 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent};
    use rail_core::config::FieldKeys;

    use super::super::TextInput;
    use crate::tui::test_util::{ctrl, key};

    /// A vi field holding `text`, in normal mode with the cursor on its last character.
    fn normal(text: &str) -> TextInput {
        let mut input = TextInput::default();
        input.set_keys(FieldKeys::Vi);
        input.set_value(text);
        assert!(input.takes_esc(), "a field opens in insert mode");
        press(&mut input, KeyCode::Esc);
        assert!(!input.takes_esc(), "Esc went to normal mode");
        input
    }

    fn press(input: &mut TextInput, code: KeyCode) -> bool {
        input.handle_key(key(code))
    }

    /// Type `keys` as normal-mode commands (or text, once one goes to insert mode).
    fn keys(input: &mut TextInput, keys: &str) {
        for c in keys.chars() {
            press(input, KeyCode::Char(c));
        }
    }

    fn at(input: &TextInput) -> (&str, usize) {
        (input.value(), input.cursor())
    }

    #[test]
    fn insert_mode_types_and_esc_steps_back_onto_the_last_character() {
        let mut input = TextInput::default();
        input.set_keys(FieldKeys::Vi);
        keys(&mut input, "hello");
        assert_eq!(at(&input), ("hello", 5));
        assert!(!press(&mut input, KeyCode::Esc));
        assert_eq!(at(&input), ("hello", 4));
        keys(&mut input, "hjkl");
        assert_eq!(at(&input), ("hello", 4), "l stops on the last character");
    }

    #[test]
    fn insert_mode_keeps_bash_vi_insert_ctrl_w_ctrl_u_and_ctrl_h() {
        let mut input = TextInput::default();
        input.set_keys(FieldKeys::Vi);
        keys(&mut input, "to me@example.com");
        assert!(input.handle_key(ctrl('w')));
        assert_eq!(input.value(), "to ");
        assert!(input.handle_key(ctrl('h')));
        assert_eq!(input.value(), "to");
        assert!(input.handle_key(ctrl('u')));
        assert_eq!(input.value(), "");
        assert!(!input.handle_key(ctrl('a')), "readline keys are not vi's");
    }

    #[test]
    fn motions_move_by_character_word_and_line() {
        let mut input = normal("re: the plan");
        keys(&mut input, "0");
        assert_eq!(input.cursor(), 0);
        for want in [2, 4, 8, 11] {
            keys(&mut input, "w");
            assert_eq!(input.cursor(), want, "w");
        }
        for want in [8, 4, 2, 0] {
            keys(&mut input, "b");
            assert_eq!(input.cursor(), want, "b");
        }
        for want in [1, 2, 6, 11] {
            keys(&mut input, "e");
            assert_eq!(input.cursor(), want, "e");
        }
        keys(&mut input, "0W");
        assert_eq!(input.cursor(), 4, "W skips the colon with its word");
        keys(&mut input, "$");
        assert_eq!(input.cursor(), 11);
        keys(&mut input, "B");
        assert_eq!(input.cursor(), 8);
        press(&mut input, KeyCode::Home);
        assert_eq!(input.cursor(), 0);
        press(&mut input, KeyCode::End);
        assert_eq!(input.cursor(), 11);

        let mut input = normal("   indented");
        keys(&mut input, "^");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn x_x_d_and_tilde_edit_in_place() {
        let mut input = normal("abcd");
        keys(&mut input, "0x");
        assert_eq!(at(&input), ("bcd", 0));
        keys(&mut input, "$X");
        assert_eq!(at(&input), ("bd", 1));
        keys(&mut input, "0~");
        assert_eq!(at(&input), ("Bd", 1));
        keys(&mut input, "0lD");
        assert_eq!(at(&input), ("B", 0));
        keys(&mut input, "x");
        assert_eq!(at(&input), ("", 0));
        assert!(!press(&mut input, KeyCode::Char('x')), "nothing to remove");
    }

    #[test]
    fn d_takes_a_motion_and_dd_takes_the_line() {
        let mut input = normal("from:alice bob");
        keys(&mut input, "0dw");
        assert_eq!(at(&input), (":alice bob", 0));
        keys(&mut input, "dW");
        assert_eq!(at(&input), ("bob", 0));
        keys(&mut input, "lde");
        assert_eq!(at(&input), ("b", 0));

        let mut input = normal("one two");
        keys(&mut input, "d0");
        assert_eq!(
            at(&input),
            ("o", 0),
            "d0 leaves the character under the cursor"
        );
        let mut input = normal("one two");
        keys(&mut input, "0d$");
        assert_eq!(at(&input), ("", 0));
        let mut input = normal("one two");
        keys(&mut input, "db");
        assert_eq!(input.value(), "one o");
        let mut input = normal("one two");
        keys(&mut input, "dd");
        assert_eq!(at(&input), ("", 0));

        let mut input = normal("keep");
        keys(&mut input, "dz");
        assert_eq!(input.value(), "keep", "an unknown motion cancels d");
        keys(&mut input, "x");
        assert_eq!(input.value(), "kee", "and x is a command again");
    }

    #[test]
    fn c_changes_and_types_in_its_place() {
        let mut input = normal("re: the plan");
        keys(&mut input, "0wwcwnew");
        assert_eq!(
            input.value(),
            "re: new plan",
            "cw stops at the end of the word"
        );
        assert!(input.takes_esc(), "c leaves the field in insert mode");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "ccall");
        assert_eq!(input.value(), "all");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "0CA");
        assert_eq!(input.value(), "A");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "sZ");
        assert_eq!(input.value(), "Z");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "Snew");
        assert_eq!(input.value(), "new");
    }

    #[test]
    fn i_a_capital_i_and_capital_a_choose_where_typing_goes() {
        let mut input = normal("bd");
        keys(&mut input, "0ac");
        assert_eq!(input.value(), "bcd");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "Ia");
        assert_eq!(input.value(), "abcd");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "Ae");
        assert_eq!(input.value(), "abcde");
        press(&mut input, KeyCode::Esc);
        keys(&mut input, "0i>");
        assert_eq!(at(&input), (">abcde", 1));
    }

    #[test]
    fn r_replaces_the_character_under_the_cursor() {
        let mut input = normal("cat");
        keys(&mut input, "0rb");
        assert_eq!(at(&input), ("bat", 0));
        assert!(!input.takes_esc(), "still in normal mode");
        keys(&mut input, "r");
        assert!(
            input.takes_esc(),
            "r waits for its character, so Esc cancels it"
        );
        press(&mut input, KeyCode::Esc);
        assert_eq!(input.value(), "bat");
        assert!(!input.takes_esc());
    }

    #[test]
    fn normal_mode_ignores_ctrl_and_alt_and_unknown_keys() {
        let mut input = normal("abc");
        assert!(!input.handle_key(ctrl('w')));
        assert!(!input.handle_key(KeyEvent::new(
            KeyCode::Char('b'),
            crossterm::event::KeyModifiers::ALT
        )));
        keys(&mut input, "qzZ");
        assert_eq!(at(&input), ("abc", 2));
    }

    #[test]
    fn masked_motions_take_the_whole_value_as_one_word() {
        let mut input = normal("GOCSPX-ab cd");
        input.set_masked(true);
        keys(&mut input, "b");
        assert_eq!(input.cursor(), 0, "stopped inside the secret");
        keys(&mut input, "w");
        assert_eq!(input.cursor(), 11, "stopped inside the secret");
        keys(&mut input, "0dw");
        assert_eq!(input.value(), "");
    }

    #[test]
    fn switching_keys_starts_over_in_insert_mode() {
        let mut input = normal("abc");
        input.set_keys(FieldKeys::Readline);
        assert!(!input.takes_esc(), "only vi fields take Esc");
        input.set_keys(FieldKeys::Vi);
        assert!(input.takes_esc());
        keys(&mut input, "x");
        assert_eq!(input.value(), "abxc", "typed, in insert mode");
    }

    #[test]
    fn words_count_characters_not_bytes() {
        let mut input = normal("café über");
        keys(&mut input, "0w");
        assert_eq!(input.cursor(), 5);
        keys(&mut input, "dw");
        assert_eq!(input.value(), "café ");
        keys(&mut input, "0e");
        assert_eq!(input.cursor(), 3);
    }
}
