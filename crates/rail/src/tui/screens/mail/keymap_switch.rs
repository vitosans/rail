//! `:keymap`: the preset picker and the switch. The new keys are built off the UI thread from
//! `config.toml` as it is now, the preset under its `[keys]`, so a switch the user's own entries
//! clash with is refused with the message start-up would give, and the keys in force stay.

use std::path::Path;
use std::sync::Arc;

use rail_core::config::{KeymapPreset, load_text_or_empty};

use super::MailScreen;
use super::themes::Saved;
use crate::tui::app::Effect;
use crate::tui::data::{MailEvent, job, send};
use crate::tui::keys::{Keymap, summary};
use crate::tui::modals::{Modal, Picker, PickerItem, PickerKind};
use crate::tui::text::escape;

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
        self.ctx.keymap = keymap.clone();
        self.ctx.keymap_preset = preset;
        self.chord.clear();
        let notice = format!("keys: {} · F2 opens the palette", preset.as_str());
        self.status.notify(escape(&notice), self.tick);
        vec![
            Effect::Keymap(keymap),
            self.save_general(
                "keymap",
                Saved::Text(Some(preset.as_str().to_string())),
                |result| MailEvent::KeymapSaved { result },
            ),
        ]
    }
}

/// The keys `preset` gives under the `[keys]` that `config.toml` at `path` holds now.
fn keymap_for(path: &Path, preset: KeymapPreset) -> Result<Arc<Keymap>, String> {
    let (text, config) = load_text_or_empty(path).map_err(|e| e.to_string())?;
    Keymap::from_config(&config.keys, preset, &text, path)
        .map(Arc::new)
        .map_err(|e| e.to_string())
}
