//! The keymap: every action's keys, the defaults with the chosen preset (`keymap`) over them
//! and `[keys]` from `config.toml` over that, key resolution (view first, then global;
//! two-key chords), and the help rows `?` shows. Built at startup and again by `:keymap`.
//! `F2` opens the palette whatever the preset and `[keys]` say, so a switch never hides it.

mod actions;
mod names;
#[cfg(test)]
mod preset_tests;
mod presets;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::{KeymapPreset, Spanned, line_col};

pub use actions::{Action, View};
pub use names::{Binding, Key, parse_binding};
pub use presets::{overrides, summary};

use crate::cmd::text::escape_field;

/// How much of a user's id or key name an error repeats.
const SHOWN_CHARS: usize = 64;

/// A first chord key waiting for its second, per screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Chord(Option<Key>);

impl Chord {
    pub fn is_pending(&self) -> bool {
        self.0.is_some()
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

/// What one key press means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Action(Action),
    /// The first key of a chord: wait for the second.
    Pending,
    Unbound,
}

/// One row of help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    pub keys: String,
    pub description: &'static str,
    pub action: Action,
    /// Set from `[keys]` rather than the default.
    pub overridden: bool,
}

/// A bad `[keys]` entry: the file, the line of the entry, and what is wrong (any of the
/// user's text in it escaped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapError {
    pub path: PathBuf,
    pub line: usize,
    pub message: String,
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

impl std::error::Error for KeymapError {}

/// True for a key that types a character in a text field: printable, without Ctrl or Alt.
pub fn is_text_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char(_))
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", escape_field(text, SHOWN_CHARS))
}

const CTRL_C: Key = Key {
    code: KeyCode::Char('c'),
    mods: KeyModifiers::CONTROL,
};

/// Opens the palette in every preset, whatever `[keys]` says.
const F2: Key = Key {
    code: KeyCode::F(2),
    mods: KeyModifiers::NONE,
};

/// Every action's keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Binding>>,
    overridden: BTreeSet<Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Keymap {
    /// The default keys of every action.
    pub fn defaults() -> Self {
        let bindings = Action::ALL
            .iter()
            .map(|&action| {
                let keys = action
                    .default_keys()
                    .iter()
                    .filter_map(|name| parse_binding(name))
                    .collect();
                (action, keys)
            })
            .collect();
        Self {
            bindings,
            overridden: BTreeSet::new(),
        }
    }

    /// The defaults with `preset` over them, then `[keys]` over that, and `F2` on the palette.
    /// `text` is `config.toml`'s text (for line numbers) and `path` its path (for messages).
    pub fn from_config(
        keys: &BTreeMap<String, Spanned<String>>,
        preset: KeymapPreset,
        text: &str,
        path: &Path,
    ) -> Result<Self, KeymapError> {
        let mut map = Self::defaults();
        for (action, names) in overrides(preset) {
            let bindings = names
                .iter()
                .filter_map(|name| parse_binding(name))
                .collect();
            map.bindings.insert(*action, bindings);
        }
        let mut lines = BTreeMap::new();
        for (id, value) in keys {
            let line = line_col(text, value.span().start).0;
            let (action, binding) =
                read_entry(id, value.get_ref()).map_err(|message| KeymapError {
                    path: path.to_path_buf(),
                    line,
                    message,
                })?;
            map.bindings.insert(action, vec![binding]);
            map.overridden.insert(action);
            lines.insert(action, line);
        }
        let palette = map.bindings.entry(Action::GlobalPalette).or_default();
        if !palette.contains(&Binding::Single(F2)) {
            palette.push(Binding::Single(F2));
        }
        map.check(&lines).map_err(|(line, message)| KeymapError {
            path: path.to_path_buf(),
            line,
            message,
        })?;
        Ok(map)
    }

    /// The first clash in any view, with the line of the entry that caused it.
    fn check(&self, lines: &BTreeMap<Action, usize>) -> Result<(), (usize, String)> {
        for view in View::ALL {
            let bound: Vec<(Action, &Binding)> = Action::ALL
                .iter()
                .filter(|a| a.views().contains(&view))
                .flat_map(|&a| self.bindings(a).iter().map(move |b| (a, b)))
                .collect();
            for (i, (first, a)) in bound.iter().enumerate() {
                for (second, b) in &bound[i + 1..] {
                    if first != second && a.clashes(b) {
                        let line = lines.get(first).max(lines.get(second)).copied();
                        return Err((
                            line.unwrap_or(0),
                            format!(
                                "[keys] {} ({a}) and {} ({b}) cannot share a key in the {} view",
                                quoted(first.id()),
                                quoted(second.id()),
                                view.name()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// What `key` means in `views` (most specific first), given a pending chord key. A
    /// Shift-arrow nothing binds there means what the plain arrow means.
    pub fn resolve(&self, views: &[View], chord: &mut Chord, key: KeyEvent) -> Resolved {
        let pressed = Key::from_event(key);
        let first = chord.0.take();
        for &view in views {
            let mut prefix = false;
            for &action in Action::ALL.iter().filter(|a| a.views().contains(&view)) {
                for binding in self.bindings(action) {
                    match (first, binding) {
                        (Some(f), Binding::Chord(a, b)) if *a == f && *b == pressed => {
                            return Resolved::Action(action);
                        }
                        (None, Binding::Single(k)) if *k == pressed => {
                            return Resolved::Action(action);
                        }
                        (None, Binding::Chord(a, _)) if *a == pressed => prefix = true,
                        _ => {}
                    }
                }
            }
            if prefix {
                chord.0 = Some(pressed);
                return Resolved::Pending;
            }
        }
        if first.is_some() {
            // The waiting key led nowhere: it is dropped and this key is read on its own.
            return self.resolve(views, chord, key);
        }
        if pressed.mods.contains(KeyModifiers::SHIFT) {
            // A Shift-arrow nothing here binds moves as the plain arrow does.
            let plain = KeyEvent::new(key.code, key.modifiers.difference(KeyModifiers::SHIFT));
            return self.resolve(views, chord, plain);
        }
        Resolved::Unbound
    }

    pub fn bindings(&self, action: Action) -> &[Binding] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    /// The action's first key as hints show it ("↑", "Enter", "Ctrl-r", "g x").
    pub fn label(&self, action: Action) -> String {
        self.bindings(action)
            .first()
            .map_or_else(|| "(unbound)".to_string(), ToString::to_string)
    }

    pub fn is_overridden(&self, action: Action) -> bool {
        self.overridden.contains(&action)
    }

    /// Help for `view`: every action that works there, with all its keys.
    pub fn help(&self, view: View) -> Vec<HelpRow> {
        Action::ALL
            .iter()
            .filter(|a| a.views().contains(&view))
            .map(|&action| HelpRow {
                keys: self
                    .bindings(action)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                description: action.description(),
                action,
                overridden: self.is_overridden(action),
            })
            .collect()
    }
}

/// One `[keys]` entry as an action and its binding, or why not.
fn read_entry(id: &str, value: &str) -> Result<(Action, Binding), String> {
    let action = Action::from_id(id).ok_or_else(|| {
        format!(
            "[keys] {} is not an action Rail knows (the README lists them)",
            quoted(id)
        )
    })?;
    let binding = parse_binding(value).ok_or_else(|| {
        format!(
            "[keys] {}: {} is not a key Rail can read",
            quoted(id),
            quoted(value)
        )
    })?;
    if binding.uses(CTRL_C) {
        return Err(format!(
            "[keys] {} is bound to Ctrl-C, which always quits Rail",
            quoted(id)
        ));
    }
    if action != Action::GlobalPalette && binding.uses(F2) {
        return Err(format!(
            "[keys] {} is bound to F2, but F2 always opens the palette",
            quoted(id)
        ));
    }
    if action == Action::GlobalQuit && matches!(binding, Binding::Chord(..)) {
        return Err(format!("[keys] {} must be one key, not two", quoted(id)));
    }
    Ok((action, binding))
}
