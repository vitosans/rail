//! The composer: a review screen over the mail screen, its prompts, and the outcomes the mail
//! screen acts on. Pure: no daemon, no store.

pub mod file;
pub mod model;
pub mod prefill;
pub mod prompt;
pub(crate) mod review;
#[cfg(test)]
mod tests;
mod view;

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};
use rail_core::config::FieldKeys;
use rail_core::ops::drafts::parse_recipient;
use rail_core::store::addresses::AddressEntry;

use self::model::{Composing, Headers};
use self::prompt::{HeaderPrompt, PathPrompt, PromptOutcome};
use crate::tui::keys::Action;

/// What the mail screen does after a key in the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeOutcome {
    Stay,
    Edit,
    Send,
    /// Save the draft to Gmail's Drafts.
    Push,
    /// Close the review and keep the draft.
    Close,
    /// Discard the draft (already confirmed); also Esc in the opening prompts of a new
    /// message, before anything was saved.
    Discard,
    /// Attach the file at this absolute path.
    Attach(String),
    /// Remove the attachment at this index.
    Remove(usize),
    /// The header prompts finished; `composing().headers` holds the new values.
    HeadersChanged,
    /// The open prompt wants address suggestions for this prefix: ask with `next_seq()`.
    SuggestionsWanted(String),
}

/// What the cursor row of the review shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Attachment(usize),
    Body,
}

enum Prompting {
    Headers(HeaderState),
    Path(PathPrompt),
}

/// The headers `t` prompts for, in order.
const HEADER_STEPS: [&str; 4] = ["To", "Cc", "Bcc", "Subject"];
/// Rows a page key moves.
const PAGE_ROWS: isize = 20;
/// The width rows are laid out at before the first draw.
const FIRST_WIDTH: usize = 80;

pub struct Composer {
    composing: Composing,
    cursor: usize,
    scroll: usize,
    prompting: Option<Prompting>,
    confirm_discard: bool,
    seq: u64,
    /// The review's width and body height at the last draw, for cursor moves.
    width: Cell<usize>,
    view_rows: Cell<usize>,
}

/// A header's current value as its prompt starts with it.
fn header_value(headers: &Headers, label: &str) -> String {
    match label {
        "To" => headers.to.join(", "),
        "Cc" => headers.cc.join(", "),
        "Bcc" => headers.bcc.join(", "),
        _ => headers.subject.clone(),
    }
}

/// Store a prompt's answer in `headers`; an address that does not parse is refused with
/// `parse_recipient`'s reason.
fn store_header(headers: &mut Headers, label: &str, value: &str) -> Result<(), String> {
    let list = match label {
        "To" => &mut headers.to,
        "Cc" => &mut headers.cc,
        "Bcc" => &mut headers.bcc,
        _ => {
            headers.subject = value.trim().to_string();
            return Ok(());
        }
    };
    let items = file::split_addresses(value);
    if let Some(Err(why)) = items
        .iter()
        .map(|i| parse_recipient(i))
        .find(Result::is_err)
    {
        return Err(format!("{}: {why}", label.to_lowercase()));
    }
    *list = items;
    Ok(())
}

fn header_prompt(steps: &[&'static str], step: usize, headers: &Headers) -> Option<HeaderPrompt> {
    let label = *steps.get(step)?;
    let value = header_value(headers, label);
    Some(HeaderPrompt::new(label, &value, label != "Subject"))
}

impl Composer {
    /// The review of `composing`.
    pub fn new(composing: Composing) -> Self {
        Self {
            composing,
            cursor: 0,
            scroll: 0,
            prompting: None,
            confirm_discard: false,
            seq: 0,
            width: Cell::new(FIRST_WIDTH),
            view_rows: Cell::new(0),
        }
    }

    /// A new message: the header prompts for `steps` first (`["To", "Subject"]`); Enter on the
    /// last gives `HeadersChanged`, Esc on any gives `Discard`.
    pub fn new_with_prompts(composing: Composing, steps: &'static [&'static str]) -> Self {
        let mut composer = Self::new(composing);
        composer.start_headers(steps, true);
        composer
    }

    pub fn composing(&self) -> &Composing {
        &self.composing
    }

    pub fn composing_mut(&mut self) -> &mut Composing {
        &mut self.composing
    }

    /// Show `text` as the last refusal on the review.
    pub fn set_error(&mut self, text: String) {
        self.composing.errors = vec![text];
    }

    /// A prompt or the discard question takes raw keys: the keymap must not resolve them.
    pub fn captures_text(&self) -> bool {
        self.prompting.is_some() || self.confirm_discard
    }

    /// Whether the discard question is open; the hint bar asks it.
    pub fn confirming_discard(&self) -> bool {
        self.confirm_discard
    }

    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The open prompt's label: a header's name, or `Attach`.
    #[cfg(test)]
    pub fn prompt_label(&self) -> Option<&'static str> {
        match &self.prompting {
            Some(Prompting::Headers(s)) => Some(s.prompt.label()),
            Some(Prompting::Path(_)) => Some("Attach"),
            None => None,
        }
    }

    /// The next sequence number for a suggestions request; earlier answers become stale.
    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Suggestions for the open `To`/`Cc`/`Bcc` prompt; ignored when `seq` is stale.
    pub fn prompt_suggestions(&mut self, seq: u64, entries: Vec<AddressEntry>) {
        if seq != self.seq {
            return;
        }
        if let Some(Prompting::Headers(s)) = &mut self.prompting {
            s.prompt.suggestions(entries);
        }
    }

    /// What the cursor row shows.
    pub fn cursor_row_kind(&self) -> RowKind {
        let rows = review::rows(&self.composing, self.width.get(), "");
        rows.get(self.cursor.min(rows.len().saturating_sub(1)))
            .map_or(RowKind::Body, |r| r.kind)
    }

    fn start_headers(&mut self, steps: &'static [&'static str], opening: bool) {
        let draft = self.composing.headers.clone();
        if let Some(prompt) = header_prompt(steps, 0, &draft) {
            self.prompting = Some(Prompting::Headers(HeaderState {
                steps,
                step: 0,
                prompt,
                draft,
                opening,
            }));
        }
    }

    /// `action` is the keymap's resolution of `key` in the `Compose` view (`None` while
    /// `captures_text`).
    /// Hand the open prompt `general.field_keys`, before a key.
    pub fn use_field_keys(&mut self, keys: FieldKeys) {
        match &mut self.prompting {
            Some(Prompting::Path(prompt)) => prompt.use_field_keys(keys),
            Some(Prompting::Headers(state)) => state.prompt.use_field_keys(keys),
            None => {}
        }
    }

    pub fn key(&mut self, action: Option<Action>, key: KeyEvent) -> ComposeOutcome {
        if self.confirm_discard {
            return self.confirm_key(key);
        }
        if self.prompting.is_some() {
            return self.prompt_key(key);
        }
        let Some(action) = action else {
            return ComposeOutcome::Stay;
        };
        self.review_action(action)
    }

    fn review_action(&mut self, action: Action) -> ComposeOutcome {
        match action {
            Action::ComposeSend => return ComposeOutcome::Send,
            Action::ComposePush => return ComposeOutcome::Push,
            Action::ComposeEdit => return ComposeOutcome::Edit,
            Action::ComposeClose => return ComposeOutcome::Close,
            Action::ComposeRemove => {
                if let RowKind::Attachment(i) = self.cursor_row_kind() {
                    return ComposeOutcome::Remove(i);
                }
            }
            Action::ComposeDiscard => self.confirm_discard = true,
            Action::ComposeAttach => self.prompting = Some(Prompting::Path(PathPrompt::new(""))),
            Action::ComposeHeaders => self.start_headers(&HEADER_STEPS, false),
            Action::ComposeDown => self.move_cursor(1),
            Action::ComposeUp => self.move_cursor(-1),
            Action::ComposePageDown => self.move_cursor(PAGE_ROWS),
            Action::ComposePageUp => self.move_cursor(-PAGE_ROWS),
            _ => {}
        }
        ComposeOutcome::Stay
    }

    /// `y` or Enter discards; `n`, Esc or `q` keeps the draft; other keys keep asking.
    fn confirm_key(&mut self, key: KeyEvent) -> ComposeOutcome {
        match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                self.confirm_discard = false;
                ComposeOutcome::Discard
            }
            KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {
                self.confirm_discard = false;
                ComposeOutcome::Stay
            }
            _ => ComposeOutcome::Stay,
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) -> ComposeOutcome {
        match self.prompting.take() {
            Some(Prompting::Path(mut prompt)) => match prompt.key(key) {
                PromptOutcome::Done(path) => ComposeOutcome::Attach(path),
                PromptOutcome::Cancel => ComposeOutcome::Stay,
                PromptOutcome::Stay | PromptOutcome::NeedSuggestions(_) => {
                    self.prompting = Some(Prompting::Path(prompt));
                    ComposeOutcome::Stay
                }
            },
            Some(Prompting::Headers(mut state)) => {
                let outcome = state.prompt.key(key);
                self.header_outcome(state, outcome)
            }
            None => ComposeOutcome::Stay,
        }
    }

    fn header_outcome(&mut self, mut s: HeaderState, outcome: PromptOutcome) -> ComposeOutcome {
        match outcome {
            PromptOutcome::Cancel if s.opening => return ComposeOutcome::Discard,
            PromptOutcome::Cancel => return ComposeOutcome::Stay,
            PromptOutcome::Done(value) => {
                if let Err(why) = store_header(&mut s.draft, s.prompt.label(), &value) {
                    s.prompt.refuse(why);
                } else if let Some(next) = header_prompt(s.steps, s.step + 1, &s.draft) {
                    s.step += 1;
                    s.prompt = next;
                } else {
                    self.composing.headers = s.draft;
                    return ComposeOutcome::HeadersChanged;
                }
            }
            PromptOutcome::NeedSuggestions(prefix) => {
                self.prompting = Some(Prompting::Headers(s));
                return ComposeOutcome::SuggestionsWanted(prefix);
            }
            PromptOutcome::Stay => {}
        }
        self.prompting = Some(Prompting::Headers(s));
        ComposeOutcome::Stay
    }

    /// Move the cursor by `delta` rows, inside the rows; the scroll follows it.
    fn move_cursor(&mut self, delta: isize) {
        let len = review::rows(&self.composing, self.width.get(), "").len();
        let last = len.saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.scroll = view::top(self.cursor, self.scroll, self.view_rows.get());
    }
}

/// The header prompts: which step is open and the values taken so far.
struct HeaderState {
    steps: &'static [&'static str],
    step: usize,
    prompt: HeaderPrompt,
    draft: Headers,
    /// The prompts a new message opens with: Esc abandons the message.
    opening: bool,
}
