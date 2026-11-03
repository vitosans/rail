//! The onboarding wizard screen. Keys become wizard `Input`s, wizard `Action`s become
//! effects, and effect results come back as wizard `Event`s. Drawing is in `wizard_view`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::FieldKeys;
use rail_core::error::ErrorClass;
use rail_core::onboarding::{
    Action as WizardAction, Body, Busy, Event, Failure, Field, FieldView, Input, PresetEntry,
    Primary, Step, ViewModel, Wizard,
};
use ratatui::Frame;
use url::Url;

use crate::tui::app::{Effect, Msg, TaskResult};
use crate::tui::effects::{EffectContext, to_effect};
use crate::tui::keys::{Action, Chord, Keymap, Resolved, View, is_text_key};
use crate::tui::screen::Screen;
use crate::tui::theme::Theme;
use crate::tui::widgets::scroll_panel::{Scroll, ScrollPanel, line_count};
use crate::tui::widgets::select::{Nav, Select, SelectOutcome};
use crate::tui::widgets::text_input::TextInput;

/// The instructions panel's text width and height before the first frame is drawn.
const FALLBACK_PANEL: (u16, u16) = (57, 10);

/// What keys mean right now.
/// The copy notice: the clipboard escape cannot confirm delivery, so it says where to look.
/// The status line is one row, so both notices stay under 80 columns.
const COPY_NOTICE: &str = "Sent via OSC 52. If not copied, select the URL on screen";
/// The consent URL is the only one printed on exit, so only its notice says so.
const COPY_CONSENT_NOTICE: &str =
    "Sent via OSC 52. If not copied, select the URL; it is also printed on quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Choices,
    Fields,
    /// Carries the instructions' height in wrapped rows.
    Instructions(u16),
    Message,
}

/// The wizard on screen. No `Debug`: the inputs and the wizard hold the client secret.
pub struct WizardScreen {
    pub(super) wizard: Wizard,
    ctx: EffectContext,
    pub(super) keymap: Arc<Keymap>,
    chord: Chord,
    pub(super) model: ViewModel,
    /// Edit state (cursor, masking) per text field; the wizard owns the values.
    pub(super) inputs: BTreeMap<Field, TextInput>,
    /// `general.field_keys`: how the fields edit.
    field_keys: FieldKeys,
    pub(super) focus: usize,
    pub(super) select: Select,
    pub(super) scroll: ScrollPanel,
    /// The instructions panel's text width and visible height from the last frame drawn,
    /// so scrolling counts rows the way the screen wraps them.
    pub(super) panel: Cell<(u16, u16)>,
    pub(super) tick: usize,
    /// The outcome of a copy, shown until the next key.
    pub(super) notice: Option<String>,
    /// A job that panicked with nothing in flight, shown until the next key.
    pub(super) failure: Option<String>,
    /// Ctrl-C arrived while saving; quit once the save reports back.
    quit_after_save: bool,
    step: Step,
}

impl WizardScreen {
    /// A screen showing `wizard`, running its actions with `ctx`.
    pub fn new(wizard: Wizard, ctx: EffectContext, keymap: Arc<Keymap>) -> Self {
        let model = wizard.view_model();
        let step = wizard.step();
        let select = Select::new(choices(&model.body));
        let mut screen = Self {
            wizard,
            ctx,
            keymap,
            chord: Chord::default(),
            model,
            inputs: BTreeMap::new(),
            field_keys: FieldKeys::default(),
            focus: 0,
            select,
            scroll: ScrollPanel::default(),
            panel: Cell::new((0, 0)),
            tick: 0,
            notice: None,
            failure: None,
            quit_after_save: false,
            step,
        };
        screen.sync_inputs();
        screen
    }

    /// Names of the accounts saved in this session.
    pub fn added(&self) -> &[String] {
        self.wizard.added()
    }

    /// True when the wizard ended on its final step, as opposed to being cancelled midway.
    pub fn finished(&self) -> bool {
        self.wizard.step() == Step::Done
    }

    /// The last consent and login URLs shown, for the caller to print once the terminal is back.
    pub fn shown_urls(&self) -> (Option<Url>, Option<Url>) {
        self.wizard.shown_urls()
    }

    /// True while the account is being written: nothing may cancel that.
    pub(super) fn saving(&self) -> bool {
        self.wizard.busy() == Some(Busy::Persisting)
    }

    fn handle_inputs(&mut self, inputs: Vec<Input>) -> Vec<Effect> {
        let mut actions = Vec::new();
        for input in inputs {
            actions.extend(self.wizard.handle(input));
        }
        self.refresh();
        self.effects(actions)
    }

    /// Apply an outcome; a quit deferred by the save happens once the save has reported.
    fn apply_event(&mut self, event: Event) -> Vec<Effect> {
        let actions = self.wizard.apply(event);
        self.refresh();
        let mut effects = self.effects(actions);
        if self.quit_after_save && !self.saving() {
            effects.push(Effect::Quit);
        }
        effects
    }

    /// A panicked job is the failure of whatever action is in flight, and that job is
    /// cancelled so it cannot deliver later. While saving, the panic is only logged: it may
    /// come from a sibling job, and the save is never abandoned. The log gets only the
    /// message's length, since a panic message can quote a token or mail text.
    fn job_panicked(&mut self, message: &str) -> Vec<Effect> {
        tracing::error!(message_bytes = message.len(), "a wizard job panicked");
        if self.saving() {
            return Vec::new();
        }
        let failure = Failure::new(
            format!("internal error: {message}; see rail.log"),
            ErrorClass::Permanent,
        );
        let event = match self.wizard.busy() {
            Some(Busy::LoadingPresets) => Event::PresetsLoaded(vec![PresetEntry {
                source: PathBuf::new(),
                preset: Err(failure.message),
            }]),
            Some(Busy::FetchingPreset) => Event::PresetFetched(Err(failure)),
            Some(Busy::Consent) => Event::AdminConsentFinished(Err(failure)),
            Some(Busy::Login) => Event::LoginFinished(Err(failure)),
            Some(Busy::Persisting) => return Vec::new(),
            Some(Busy::SavingKeys) => Event::KeymapSaved(Err(failure)),
            None => {
                self.failure = Some(format!("Error: {}", failure.message));
                return Vec::new();
            }
        };
        let mut effects = vec![Effect::CancelAll];
        effects.extend(self.apply_event(event));
        effects
    }

    fn copied(&mut self, result: Result<(), String>) {
        // Only the consent URL survives a quit; the login URL dies with this process.
        let consent = self.shows_consent_url();
        self.notice = Some(match result {
            Ok(()) if consent => COPY_CONSENT_NOTICE.to_string(),
            Ok(()) => COPY_NOTICE.to_string(),
            Err(e) => format!("Could not copy: {e}; the URL is shown above"),
        });
    }

    fn effects(&self, actions: Vec<WizardAction>) -> Vec<Effect> {
        actions
            .into_iter()
            .map(|action| to_effect(action, &self.ctx))
            .collect()
    }

    /// Re-read the view model; a new step resets focus, scroll and the list selection.
    fn refresh(&mut self) {
        self.model = self.wizard.view_model();
        let items = choices(&self.model.body);
        if self.wizard.step() == self.step {
            self.select.set_items(items);
        } else {
            self.step = self.wizard.step();
            self.focus = 0;
            self.scroll = ScrollPanel::default();
            self.select = Select::new(items);
        }
        self.sync_inputs();
    }

    /// Text the wizard changed itself (a derived name, a preset's tenant) reaches the inputs.
    /// Edit the fields with `keys`, `general.field_keys` from the config being added to.
    pub fn with_field_keys(mut self, keys: FieldKeys) -> Self {
        self.field_keys = keys;
        self.inputs
            .values_mut()
            .for_each(|input| input.set_keys(keys));
        self
    }

    fn sync_inputs(&mut self) {
        for view in text_fields(&self.model.body) {
            let input = self.inputs.entry(view.field).or_default();
            input.set_keys(self.field_keys);
            input.set_masked(view.masked);
            if input.value() != view.value {
                input.set_value(&view.value);
            }
        }
    }

    pub(super) fn focused_field(&self) -> Option<&FieldView> {
        match &self.model.body {
            Body::Fields(fields) => fields.get(self.focus),
            Body::Presets { source, .. } if self.focus == 1 => Some(source),
            _ => None,
        }
    }

    /// The instructions panel's text width and visible height.
    fn panel_size(&self) -> (u16, u16) {
        match self.panel.get() {
            (0, _) | (_, 0) => FALLBACK_PANEL,
            size => size,
        }
    }

    pub(super) fn mode(&self) -> Mode {
        match &self.model.body {
            Body::Choices(_) | Body::Report { .. } => Mode::Choices,
            Body::Presets { .. } if self.focus == 0 => Mode::Choices,
            Body::Presets { .. } | Body::Fields(_) => Mode::Fields,
            Body::Instructions { text, .. } => {
                Mode::Instructions(line_count(text, self.panel_size().0))
            }
            Body::Message(_) => Mode::Message,
        }
    }

    fn focus_count(&self) -> usize {
        match &self.model.body {
            Body::Fields(fields) => fields.len().max(1),
            Body::Presets { .. } => 2,
            _ => 1,
        }
    }

    /// True when the focused field keeps `key` for itself: `Esc` in a vi field's insert
    /// mode goes to normal mode, not back a step.
    fn field_takes(&self, key: KeyEvent) -> bool {
        key.code == KeyCode::Esc
            && self
                .focused_field()
                .and_then(|view| self.inputs.get(&view.field))
                .is_some_and(TextInput::takes_esc)
    }

    /// The wizard action `key` means, unless it types text in the focused field.
    fn action(&mut self, key: KeyEvent) -> Option<Action> {
        if self.captures_text() && (is_text_key(key) || self.field_takes(key)) {
            return None;
        }
        match self.keymap.resolve(&[View::Wizard], &mut self.chord, key) {
            Resolved::Action(action) => Some(action),
            Resolved::Pending | Resolved::Unbound => None,
        }
    }

    fn on_key(&mut self, key: KeyEvent) -> Vec<Input> {
        self.notice = None;
        self.failure = None;
        if self.saving() {
            self.quit_after_save |= is_ctrl_c(key);
            return Vec::new();
        }
        let action = self.action(key);
        match action {
            Some(Action::WizardBack) => return vec![Input::Back],
            Some(Action::WizardPortal) => return vec![Input::OpenPortal],
            Some(step @ (Action::WizardNextField | Action::WizardPrevField)) => {
                self.move_focus(step == Action::WizardNextField);
                return Vec::new();
            }
            _ => {}
        }
        if !self.captures_text()
            && let Some(input) = action.and_then(|a| self.failure_input(a))
        {
            return vec![input];
        }
        match self.mode() {
            Mode::Choices => self.choose(action),
            Mode::Fields => self.field_key(key, action),
            Mode::Instructions(lines) => self.instructions(action, lines),
            Mode::Message if action == Some(Action::WizardNext) => vec![Input::Next],
            Mode::Message => Vec::new(),
        }
    }

    fn failure_input(&self, action: Action) -> Option<Input> {
        match action {
            Action::WizardRetry if self.model.failure.is_some() => Some(Input::Retry),
            Action::WizardCopyUrl if self.can_copy_url() => Some(Input::CopyConsentUrl),
            _ => None,
        }
    }

    /// True while a failure offers the consent URL to copy.
    fn shows_consent_url(&self) -> bool {
        (self.model.failure.as_ref()).is_some_and(|f| matches!(f.primary, Primary::CopyUrl(_)))
    }

    /// The consent URL of a failure, or the login URL while the Login step shows one.
    fn can_copy_url(&self) -> bool {
        let consent = self.shows_consent_url();
        consent || (self.step == Step::Login && self.model.url.is_some())
    }

    fn choose(&mut self, action: Option<Action>) -> Vec<Input> {
        let nav = match action {
            Some(Action::WizardUp) => Nav::Up,
            Some(Action::WizardDown) => Nav::Down,
            Some(Action::WizardNext) => Nav::Choose,
            _ => return Vec::new(),
        };
        match self.select.handle(nav) {
            SelectOutcome::Chosen(index) => vec![Input::Choose(index)],
            SelectOutcome::Moved | SelectOutcome::Ignored => Vec::new(),
        }
    }

    fn instructions(&mut self, action: Option<Action>, lines: u16) -> Vec<Input> {
        let step = match action {
            Some(Action::WizardNext) => return vec![Input::Next],
            Some(Action::WizardOpenPortal) => return vec![Input::OpenPortal],
            Some(Action::WizardUp) => Scroll::Up,
            Some(Action::WizardDown) => Scroll::Down,
            Some(Action::WizardPageUp) => Scroll::PageUp,
            Some(Action::WizardPageDown) => Scroll::PageDown,
            Some(Action::WizardTop) => Scroll::Top,
            Some(Action::WizardBottom) => Scroll::Bottom,
            _ => return Vec::new(),
        };
        let page = self.panel_size().1;
        self.scroll.scroll(step, lines, page);
        Vec::new()
    }

    /// In a field, characters are text (or nothing, on the checkbox) and only the arrow keys
    /// move between fields, as before the keymap: `j` and `k` are letters there.
    fn field_key(&mut self, key: KeyEvent, action: Option<Action>) -> Vec<Input> {
        let Some(view) = self.focused_field() else {
            return Vec::new();
        };
        let (field, checkbox) = (view.field, view.checkbox.is_some());
        let typed = is_text_key(key);
        match action {
            Some(Action::WizardNext) => vec![Input::Next],
            Some(step @ (Action::WizardUp | Action::WizardDown)) if !typed => {
                self.move_focus(step == Action::WizardDown);
                Vec::new()
            }
            Some(Action::WizardToggle) if checkbox => vec![Input::ToggleAdmin],
            _ if checkbox => Vec::new(),
            _ => self.edit(field, key),
        }
    }

    fn edit(&mut self, field: Field, key: KeyEvent) -> Vec<Input> {
        let input = self.inputs.entry(field).or_default();
        if input.handle_key(key) {
            vec![Input::Edit(field, input.value().to_string())]
        } else {
            Vec::new()
        }
    }

    fn move_focus(&mut self, forward: bool) {
        let count = self.focus_count();
        self.focus = if forward {
            (self.focus + 1) % count
        } else {
            (self.focus + count - 1) % count
        };
    }
}

impl Screen for WizardScreen {
    fn update(&mut self, msg: Msg) -> Vec<Effect> {
        match msg {
            Msg::Key(key) => {
                let inputs = self.on_key(key);
                self.handle_inputs(inputs)
            }
            Msg::Tick => {
                self.tick = self.tick.wrapping_add(1);
                Vec::new()
            }
            Msg::Resize(..) => Vec::new(),
            Msg::Task(TaskResult::Wizard(event)) => self.apply_event(event),
            Msg::Task(TaskResult::Mail(_) | TaskResult::EditorDone(_)) => Vec::new(),
            Msg::Task(TaskResult::JobPanicked(message)) => self.job_panicked(&message),
            Msg::Task(TaskResult::Copied(result)) => {
                self.copied(result);
                Vec::new()
            }
        }
    }

    fn view(&self, frame: &mut Frame, theme: &Theme) {
        self.render(frame, theme);
    }

    fn captures_text(&self) -> bool {
        self.focused_field()
            .is_some_and(|view| view.checkbox.is_none())
    }

    fn defers_quit(&self) -> bool {
        self.saving()
    }

    fn claims_key(&self, key: KeyEvent) -> bool {
        let mut chord = self.chord;
        self.chord.is_pending()
            || !matches!(
                self.keymap.resolve(&[View::Wizard], &mut chord, key),
                Resolved::Unbound
            )
    }
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn choices(body: &Body) -> Vec<String> {
    match body {
        Body::Choices(items)
        | Body::Presets { choices: items, .. }
        | Body::Report { choices: items, .. } => items.clone(),
        _ => Vec::new(),
    }
}

fn text_fields(body: &Body) -> Vec<FieldView> {
    match body {
        Body::Fields(fields) => fields
            .iter()
            .filter(|view| view.checkbox.is_none())
            .cloned()
            .collect(),
        Body::Presets { source, .. } => vec![source.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
