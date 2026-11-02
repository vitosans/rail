//! `:theme` and `:accent`: the pickers, the preview, the save (which `:hints` shares).

use std::path::Path;

use rail_core::config::lock::lock_config;
use rail_core::config::{
    load_text_or_empty, set_general, set_general_bool, set_general_number, set_general_text,
};
use rail_core::paths::{create_private_dir, replace_private_atomic};
use ratatui::style::Color;

use super::MailScreen;
use crate::tui::app::Effect;
use crate::tui::data::{MailEvent, job, send};
use crate::tui::modals::{AccentAnswer, Modal, Picker, PickerItem, PickerKind};
use crate::tui::text::escape;
use crate::tui::theme::Theme;
use crate::tui::theme::choice::{Accent, ColorDepth, ThemeChoice};
use crate::tui::theme::scheme::{Scheme, schemes};

/// Shown at startup on a terminal without truecolor when a theme is configured, and when a
/// picker opens there.
pub(super) const NEEDS_TRUECOLOR: &str = "theme needs a truecolor terminal";
/// The conventional names of the 16 ANSI slots.
const SLOT_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright black",
    "bright red",
    "bright green",
    "bright yellow",
    "bright blue",
    "bright magenta",
    "bright cyan",
    "bright white",
];

#[derive(Debug, Default)]
pub(crate) struct ThemeState {
    /// What is kept: the configured choice, then whatever Enter chose.
    pub(super) current: ThemeChoice,
    /// `current` as it was when a picker opened; restored on Esc.
    pub(super) before: Option<ThemeChoice>,
}

impl ThemeState {
    pub(super) fn new(current: ThemeChoice) -> Self {
        Self {
            current,
            before: None,
        }
    }
}

impl MailScreen {
    /// The theme for `choice` at this terminal's depth.
    fn theme_for(&self, choice: ThemeChoice) -> Theme {
        Theme::from_choice(choice, self.ctx.depth)
    }

    pub(super) fn open_theme_picker(&mut self) -> Vec<Effect> {
        let current = self.themes.current.scheme.map(|s| s.name);
        let mut items = vec![PickerItem {
            id: -1,
            name: "Rail default".into(),
            checked: current.is_none(),
            swatch: Vec::new(),
        }];
        items.extend(schemes().iter().enumerate().map(|(i, s)| PickerItem {
            id: i64::try_from(i).unwrap_or(i64::MAX),
            name: s.name.to_string(),
            checked: Some(s.name) == current,
            swatch: self.swatch(s),
        }));
        self.show_picker(PickerKind::Theme, items)
    }

    pub(super) fn open_accent_picker(&mut self) -> Vec<Effect> {
        let scheme = self.themes.current.scheme;
        let accent = self.themes.current.accent;
        let mut items = vec![PickerItem {
            id: -1,
            name: "scheme default".into(),
            checked: accent == Accent::SchemeDefault,
            swatch: Vec::new(),
        }];
        items.extend((0..16u8).map(|n| PickerItem {
            id: i64::from(n),
            name: format!("{n} {}", slot_name(n)),
            checked: accent == Accent::Slot(n),
            swatch: self.slot_swatch(scheme, n),
        }));
        self.show_picker(PickerKind::Accent, items)
    }

    /// Open `kind`. Without truecolor a scheme cannot be shown: the theme picker says so and
    /// does not preview; slot accents still do.
    fn show_picker(&mut self, kind: PickerKind, items: Vec<PickerItem>) -> Vec<Effect> {
        self.themes.before = Some(self.themes.current);
        let mut picker = Picker::new(kind, items);
        if kind == PickerKind::Theme && self.ctx.depth == ColorDepth::Ansi {
            picker = picker.without_preview();
            self.status.notify(NEEDS_TRUECOLOR, self.tick);
        }
        self.modal = Some(Modal::Picker(picker));
        Vec::new()
    }

    /// Three cells: the scheme's background, foreground and cyan; none without truecolor.
    fn swatch(&self, scheme: &Scheme) -> Vec<Color> {
        if self.ctx.depth == ColorDepth::Ansi {
            return Vec::new();
        }
        vec![
            scheme.background.color(),
            scheme.foreground.color(),
            scheme.palette[6].color(),
        ]
    }

    /// The slot's colour in the scheme in force, or the terminal's own slot.
    fn slot_swatch(&self, scheme: Option<&Scheme>, slot: u8) -> Vec<Color> {
        match (self.ctx.depth, scheme) {
            (ColorDepth::TrueColor, Some(s)) => vec![s.palette[usize::from(slot) % 16].color()],
            _ => vec![Color::Indexed(slot)],
        }
    }

    /// The theme picker's highlight moved. Without truecolor the scheme is ignored, so there
    /// is nothing to show.
    pub(super) fn preview_theme(&mut self, index: Option<usize>) -> Vec<Effect> {
        if self.ctx.depth == ColorDepth::Ansi {
            return Vec::new();
        }
        let choice = self.choice_for_theme(index);
        vec![Effect::Theme(self.theme_for(choice))]
    }

    /// The accent picker's highlight moved.
    pub(super) fn preview_accent(&mut self, answer: AccentAnswer) -> Vec<Effect> {
        let choice = self.choice_for_accent(answer.into());
        vec![Effect::Theme(self.theme_for(choice))]
    }

    /// Esc: back to what was kept.
    pub(super) fn restore_theme(&mut self) -> Vec<Effect> {
        match self.themes.before.take() {
            Some(before) => vec![Effect::Theme(self.theme_for(before))],
            None => Vec::new(),
        }
    }

    /// Enter in `:theme`: keep the scheme of item `index`, written as `theme`.
    pub(super) fn keep_scheme(&mut self, index: Option<usize>) -> Vec<Effect> {
        let choice = self.choice_for_theme(index);
        let name = choice.scheme.map(|s| s.name);
        let notice = format!("theme: {}", name.unwrap_or("Rail default"));
        let value = Saved::Text(name.map(str::to_string));
        self.keep(choice, &notice, "theme", value)
    }

    /// Enter in `:accent`: keep `answer`, written as `accent`.
    pub(super) fn keep_accent(&mut self, answer: AccentAnswer) -> Vec<Effect> {
        let (notice, value) = match answer {
            AccentAnswer::SchemeDefault => ("accent: scheme default".to_string(), None),
            AccentAnswer::Slot(n) => (format!("accent: {n} {}", slot_name(n)), Some(i64::from(n))),
        };
        let choice = self.choice_for_accent(answer.into());
        self.keep(choice, &notice, "accent", Saved::Number(value))
    }

    /// Keep `choice`, say `notice`, and write `value` as `key` to config.
    fn keep(
        &mut self,
        choice: ThemeChoice,
        notice: &str,
        key: &'static str,
        value: Saved,
    ) -> Vec<Effect> {
        self.modal = None;
        self.themes.before = None;
        self.themes.current = choice;
        self.status.notify(escape(notice), self.tick);
        vec![
            Effect::Theme(self.theme_for(choice)),
            self.save_general(key, value, |result| MailEvent::ThemeSaved { result }),
        ]
    }

    /// A job that writes `value` as `key` to config and answers with `done`'s event.
    pub(super) fn save_general(
        &self,
        key: &'static str,
        value: Saved,
        done: fn(Result<(), String>) -> MailEvent,
    ) -> Effect {
        let path = self.ctx.paths.config_file();
        job(move |sink| async move {
            let result = tokio::task::spawn_blocking(move || write_general(&path, key, value))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            send(&sink, done(result));
        })
    }

    /// The choice the theme picker's item `index` stands for, with the accent in force.
    pub(super) fn choice_for_theme(&self, index: Option<usize>) -> ThemeChoice {
        ThemeChoice {
            scheme: index.and_then(|i| schemes().get(i)),
            accent: self.themes.current.accent,
        }
    }

    /// `accent` over the scheme in force.
    pub(super) fn choice_for_accent(&self, accent: Accent) -> ThemeChoice {
        ThemeChoice {
            scheme: self.themes.current.scheme,
            accent,
        }
    }

    /// Say `notice` (when there is one) and write `value` as `key`: the shape every toggle
    /// shares; `done` names the event the write answers with.
    pub(super) fn flip_and_save(
        &mut self,
        notice: Option<String>,
        key: &'static str,
        value: Saved,
        done: fn(Result<(), String>) -> MailEvent,
    ) -> Vec<Effect> {
        if let Some(text) = notice {
            self.status.notify(text, self.tick);
        }
        vec![self.save_general(key, value, done)]
    }

    /// A config write answered: a failure keeps `what` (`theme`, `hints setting`, `clock
    /// setting`, `time setting`, `thread order`) for the session and says so.
    pub(super) fn setting_saved(&mut self, what: &str, result: Result<(), String>) -> Vec<Effect> {
        if let Err(reason) = result {
            tracing::warn!("could not save the {what} to config.toml");
            let text = format!("Could not save the {what}: {}", escape(&reason));
            self.status.error(text, self.ctx.clock.now(), self.tick);
        }
        Vec::new()
    }
}

/// The conventional name of ANSI slot `n`.
fn slot_name(n: u8) -> &'static str {
    SLOT_NAMES[usize::from(n) % SLOT_NAMES.len()]
}

/// What is saved: `theme`, `time` and `thread_order` are text, `accent` a number, `hints` a
/// boolean; `None` removes the key. `InList` adds (`true`) or removes one entry of a list as
/// the file has it, so entries written elsewhere stay.
pub(crate) enum Saved {
    Text(Option<String>),
    Number(Option<i64>),
    Bool(Option<bool>),
    InList(String, bool),
}

/// Read, edit and replace `config.toml` (an owner-only sibling, synced, then renamed) under the
/// config lock, which the daemon and other TUIs hold for their own edits. A symlinked file is
/// replaced where it points, so the link survives. The directory is created when missing and
/// otherwise left as it is.
pub(crate) fn write_general(path: &Path, key: &str, value: Saved) -> Result<(), String> {
    let _lock = lock_config(path).map_err(|e| e.to_string())?;
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    let target = if is_link {
        std::fs::canonicalize(path).map_err(|e| e.to_string())?
    } else {
        path.to_path_buf()
    };
    let path = target.as_path();
    let (text, _) = load_text_or_empty(path).map_err(|e| e.to_string())?;
    let out = match value {
        Saved::Text(v) => set_general_text(&text, path, key, v.as_deref()),
        Saved::Number(v) => set_general_number(&text, path, key, v),
        Saved::Bool(v) => set_general_bool(&text, path, key, v),
        Saved::InList(item, add) => set_general(&text, path, key, edited(&text, key, &item, add)),
    }
    .map_err(|e| e.to_string())?;
    if let Some(dir) = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty() && !d.is_dir())
    {
        create_private_dir(dir).map_err(|e| e.to_string())?;
    }
    replace_private_atomic(path, out.as_bytes()).map_err(|e| e.to_string())
}

/// `[general] key`'s list in `text` with `item` added when missing or removed (matched in
/// lower case), the other entries and their formatting kept; `None` once it is empty.
fn edited(text: &str, key: &str, item: &str, add: bool) -> Option<toml_edit::Value> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok();
    let mut list = doc
        .as_ref()
        .and_then(|d| d.get("general")?.get(key)?.as_array().cloned())
        .unwrap_or_default();
    let same = |v: &toml_edit::Value| v.as_str().is_some_and(|s| s.to_lowercase() == item);
    if !add {
        list.retain(|v| !same(v));
    } else if !list.iter().any(same) {
        list.push(item);
    }
    (!list.is_empty()).then(|| list.into())
}
