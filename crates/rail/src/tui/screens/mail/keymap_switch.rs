//! `:keymap`: the preset picker and the switch. The new keys are built off the UI thread from
//! `config.toml` as it is now, the preset under its `[keys]`, so a switch the user's own entries
//! clash with is refused with the message start-up would give, and the keys in force stay.
//!
//! A switch is tried before it is saved: the new keys take effect at once and "Keep these
//! keys?" asks, counting down. `y` or `Enter` keeps and saves them; `n`, `Esc`, or the time
//! running out goes back to the keys before, with nothing saved. The question's own keys are
//! fixed, so keys that are new to someone cannot strand them.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rail_core::config::{KeymapPreset, load_text_or_empty};

use super::MailScreen;
use super::themes::Saved;
use crate::tui::app::Effect;
use crate::tui::data::{MailEvent, job, send};
use crate::tui::keys::{Keymap, summary};
use crate::tui::modals::{Command, Confirm, Modal, Picker, PickerItem, PickerKind};
use crate::tui::run::TICK;
use crate::tui::text::escape;

/// How long "Keep these keys?" waits before going back.
const KEEP_KEYS: Duration = Duration::from_secs(15);

/// Ticks in a second, and in `KEEP_KEYS`.
const TICKS_PER_SECOND: usize = (1000 / TICK.as_millis()) as usize;
pub(super) const KEEP_KEYS_TICKS: usize = KEEP_KEYS.as_secs() as usize * TICKS_PER_SECOND;

/// A switch being tried: the keys to go back to, and the tick the question runs out on.
pub(super) struct KeymapTrial {
    pub(super) previous: Arc<Keymap>,
    previous_preset: KeymapPreset,
    deadline: usize,
}

impl MailScreen {
    /// Every preset with what sets it apart, the one in force marked.
    pub(super) fn open_keymap_picker(&mut self) -> Vec<Effect> {
        let items = KeymapPreset::ALL
            .iter()
            .enumerate()
            .map(|(i, &preset)| PickerItem {
                id: i64::try_from(i).unwrap_or(i64::MAX),
                name: format!("{} — {}", preset.as_str(), summary(preset)),
                checked: preset == self.ctx.keymap_preset,
                swatch: Vec::new(),
            })
            .collect();
        self.modal = Some(Modal::Picker(Picker::new(PickerKind::Keymap, items)));
        Vec::new()
    }

    /// Enter in `:keymap`: build the keys of the preset at `index` off the UI thread, since that
    /// reads `config.toml`; `keymap_built` switches once they are ready.
    pub(super) fn switch_keymap(&mut self, index: usize) -> Vec<Effect> {
        self.modal = None;
        let Some(&preset) = KeymapPreset::ALL.get(index) else {
            return Vec::new();
        };
        let path = self.ctx.paths.config_file();
        vec![job(move |sink| async move {
            let result = tokio::task::spawn_blocking(move || keymap_for(&path, preset))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            send(&sink, MailEvent::KeymapBuilt { preset, result });
        })]
    }

    /// The keys of `preset` are ready: switch to them and write `keymap`, or, when `[keys]`
    /// clashes with the preset, say so and keep the keys in force.
    pub(super) fn keymap_built(
        &mut self,
        preset: KeymapPreset,
        result: Result<Arc<Keymap>, String>,
    ) -> Vec<Effect> {
        let keymap = match result {
            Ok(keymap) => keymap,
            Err(reason) => {
                tracing::warn!("a key preset was refused: [keys] clashes with it");
                let text = format!("Keys not changed: {}", escape(&reason));
                self.status.error(text, self.ctx.clock.now(), self.tick);
                return Vec::new();
            }
        };
        let previous = self.use_keymap(keymap.clone(), preset);
        if previous.1 == preset {
            // The preset in force, read again with `[keys]` as it is now: nothing to try.
            self.status
                .notify(format!("keys: {}", preset.as_str()), self.tick);
            return vec![Effect::Keymap(keymap)];
        }
        self.keymap_trial = Some(KeymapTrial {
            previous: previous.0,
            previous_preset: previous.1,
            deadline: self.tick.wrapping_add(KEEP_KEYS_TICKS),
        });
        self.ask_to_keep_keys();
        vec![Effect::Keymap(keymap)]
    }

    /// Put `keymap` in force; the keys and preset it replaces.
    fn use_keymap(
        &mut self,
        keymap: Arc<Keymap>,
        preset: KeymapPreset,
    ) -> (Arc<Keymap>, KeymapPreset) {
        let previous = std::mem::replace(&mut self.ctx.keymap, keymap);
        let previous_preset = std::mem::replace(&mut self.ctx.keymap_preset, preset);
        self.chord.clear();
        (previous, previous_preset)
    }

    /// Open, or redraw with the time left, "Keep these keys?".
    fn ask_to_keep_keys(&mut self) {
        let Some(trial) = &self.keymap_trial else {
            return;
        };
        let left = trial
            .deadline
            .saturating_sub(self.tick)
            .div_ceil(TICKS_PER_SECOND);
        let lines = vec![
            format!(
                "Rail is using the {} keys now.",
                self.ctx.keymap_preset.as_str()
            ),
            format!(
                "Going back to the {} keys in {left} s.",
                trial.previous_preset.as_str()
            ),
        ];
        self.modal = Some(Modal::Confirm(Confirm::new(
            "Keep these keys?",
            lines,
            Command::KeepKeymap,
        )));
    }

    /// Each tick of a trial: count down, and go back once the time is up.
    pub(super) fn keymap_trial_tick(&mut self) -> Vec<Effect> {
        let Some(trial) = &self.keymap_trial else {
            return Vec::new();
        };
        let asking = matches!(self.modal, Some(Modal::Confirm(_)));
        if self.tick >= trial.deadline {
            if asking {
                self.modal = None;
            }
            return self.revert_keymap();
        }
        if asking {
            self.ask_to_keep_keys();
        }
        Vec::new()
    }

    /// "Keep these keys?" answered yes: save the preset in force.
    pub(super) fn keep_keymap(&mut self) -> Vec<Effect> {
        self.modal = None;
        if self.keymap_trial.take().is_none() {
            return Vec::new();
        }
        let preset = self.ctx.keymap_preset;
        let notice = format!("keys: {} · F2 opens the palette", preset.as_str());
        self.status.notify(escape(&notice), self.tick);
        vec![self.save_general(
            "keymap",
            Saved::Text(Some(preset.as_str().to_string())),
            |result| MailEvent::KeymapSaved { result },
        )]
    }

    /// Go back to the keys before the switch being tried, if one is; nothing was saved.
    pub(super) fn revert_keymap(&mut self) -> Vec<Effect> {
        let Some(trial) = self.keymap_trial.take() else {
            return Vec::new();
        };
        let previous = trial.previous.clone();
        self.use_keymap(trial.previous, trial.previous_preset);
        let notice = format!("keys: {} kept", trial.previous_preset.as_str());
        self.status.notify(notice, self.tick);
        vec![Effect::Keymap(previous)]
    }
}

/// The keys `preset` gives under the `[keys]` that `config.toml` at `path` holds now.
fn keymap_for(path: &Path, preset: KeymapPreset) -> Result<Arc<Keymap>, String> {
    let (text, config) = load_text_or_empty(path).map_err(|e| e.to_string())?;
    Keymap::from_config(&config.keys, preset, &text, path)
        .map(Arc::new)
        .map_err(|e| e.to_string())
}
