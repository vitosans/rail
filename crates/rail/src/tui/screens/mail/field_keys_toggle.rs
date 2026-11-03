//! `:field-keys`: switch how text fields edit and save the choice.

use super::MailScreen;
use super::themes::Saved;
use crate::tui::app::Effect;
use crate::tui::data::MailEvent;

impl MailScreen {
    pub(super) fn cycle_field_keys(&mut self) -> Vec<Effect> {
        self.ctx.field_keys = self.ctx.field_keys.next();
        let mode = self.ctx.field_keys;
        let value =
            (mode != rail_core::config::FieldKeys::default()).then(|| mode.as_str().to_string());
        self.flip_and_save(
            Some(format!("field keys: {}", mode.as_str())),
            "field_keys",
            Saved::Text(value),
            |result| MailEvent::FieldKeysSaved { result },
        )
    }
}
