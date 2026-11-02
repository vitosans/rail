//! The Drafts folder from the mail screen's side: its reads, its keys (thread actions refused),
//! Enter resuming a local draft or importing a provider one, `d` discarding either kind, and
//! `:drafts`, which lists the same rows for the scope's accounts.

use rail_core::config::AccountKind;
use rail_core::ops::{DraftImportResponse, OpError, client_token};
use rail_core::store::drafts_folder::{DraftsRow, DraftsRowKind};

use super::composing::{BUSY, Pending};
use super::drafts_pane::DraftsPane;
use super::{Focus, MailScreen, error_line};
use crate::tui::app::Effect;
use crate::tui::data::{ReadError, ScopeChoice};
use crate::tui::keys::Action;
use crate::tui::modals::{Command, Confirm, Modal, Picker, PickerItem, PickerKind};
use crate::tui::screens::compose::prefill;

/// Said for a thread action pressed in the Drafts folder.
pub(super) const NOT_ON_DRAFTS: &str = "Not available on drafts";

/// What an imported provider draft is for once the daemon answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterImport {
    /// Open its review.
    Open,
    /// Discard it (and so its provider copy).
    Discard,
}

#[derive(Debug, Default)]
pub(crate) struct DraftsState {
    pub(super) pane: DraftsPane,
    /// The newest pane read: an older answer is dropped.
    latest: u64,
    /// The rows `:drafts` lists, by the picker's item ids.
    pub(super) picked: Vec<DraftsRow>,
    /// The row a yes on the discard question applies to.
    discarding: Option<DraftsRow>,
}

impl MailScreen {
    /// The scope is the merged Drafts or an account's Drafts folder: the list area shows the
    /// drafts pane.
    pub(super) fn in_drafts(&self) -> bool {
        match &self.scope {
            ScopeChoice::Drafts => true,
            ScopeChoice::Folder { folder_id, .. } => {
                self.sidebar.drafts_folder_ids().contains(folder_id)
            }
            _ => false,
        }
    }

    /// The store ids of the scope's account (every account's in a merged mailbox).
    pub(super) fn scope_account_ids(&self) -> Vec<i64> {
        match &self.scope {
            ScopeChoice::Account(index) | ScopeChoice::Folder { account: index, .. } => self
                .account_ids
                .get(*index)
                .copied()
                .flatten()
                .into_iter()
                .collect(),
            _ => self.account_ids.iter().flatten().copied().collect(),
        }
    }

    /// Read the pane's rows again.
    pub(super) fn read_drafts_pane(&mut self) -> Effect {
        let seq = self.next_seq();
        self.drafts.latest = seq;
        self.ctx.store.drafts_folder(seq, self.scope_account_ids())
    }

    /// The Drafts folder was opened: a thread of the old scope closes, so no key acts on it.
    pub(super) fn open_drafts_pane(&mut self) -> Vec<Effect> {
        let mut effects = if self.reader.is_some() {
            self.close_reader()
        } else {
            Vec::new()
        };
        self.drafts.pane = DraftsPane::opened();
        effects.push(self.read_drafts_pane());
        effects
    }

    /// The list's keys while it shows the Drafts folder; `None` for the keys that work there
    /// as everywhere.
    pub(super) fn drafts_action(&mut self, action: Action) -> Option<Vec<Effect>> {
        if self.focus != Focus::List || !self.in_drafts() {
            return None;
        }
        let page = isize::try_from(self.height()).unwrap_or(isize::MAX);
        let pane = &mut self.drafts.pane;
        match action {
            Action::ListOpen => {
                let row = pane.selected().cloned();
                return Some(row.map_or_else(Vec::new, |row| self.open_draft_row(row)));
            }
            // A discarded draft is gone for good, so trash without asking asks here too.
            Action::ListTrash | Action::ListTrashNow => return Some(self.confirm_discard_row()),
            // Nothing is selected in the pane: extending moves, as `j`/`k` do.
            Action::ListDown | Action::ListExtendDown => pane.move_by(1),
            Action::ListUp | Action::ListExtendUp => pane.move_by(-1),
            Action::ListPageDown => pane.move_by(page),
            Action::ListPageUp => pane.move_by(-page),
            Action::ListTop => pane.top(),
            Action::ListBottom => pane.bottom(),
            // Nothing is selected in the pane: there is nothing to clear.
            Action::ListClearSelection => {}
            Action::ListArchive
            | Action::ListStar
            | Action::ListRead
            | Action::ListLabels
            | Action::ListMove
            | Action::ListSnooze
            | Action::ListSelect
            | Action::ListSelectAll
            | Action::ListSelectSender
            | Action::ListSelectRange
            | Action::ListReply
            | Action::ListReplyAll
            | Action::ListForward
            | Action::ListFilter => self.status.notify(NOT_ON_DRAFTS, self.tick),
            _ => return None,
        }
        Some(Vec::new())
    }

    /// A message is being written or a composing request is in flight: say so.
    fn composing_busy(&mut self) -> bool {
        let busy = self.compose.is_some() || self.composing.pending.is_some();
        if busy {
            self.status.notify(BUSY, self.tick);
        }
        busy
    }

    /// Enter on a row of the pane or of `:drafts`: resume a local draft; import a provider
    /// one, then resume it.
    pub(super) fn open_draft_row(&mut self, row: DraftsRow) -> Vec<Effect> {
        if self.composing_busy() {
            return Vec::new();
        }
        match row.kind {
            DraftsRowKind::Local { draft, .. } => self.resume_draft(draft),
            DraftsRowKind::Remote { message } => {
                self.import_row(row.account_id, message, AfterImport::Open)
            }
        }
    }

    fn import_row(&mut self, account_id: i64, message: i64, then: AfterImport) -> Vec<Effect> {
        let Some(index) = self.account_index_of(account_id) else {
            let text = "This draft's account is no longer configured.";
            self.status.notify(text, self.tick);
            return Vec::new();
        };
        let account = self.accounts[index].name.clone();
        let seq = self.next_compose_seq();
        self.composing.pending = Some(Pending::Importing { seq, then });
        let notice = match then {
            AfterImport::Open => "fetching the draft…",
            AfterImport::Discard => "discarding the draft…",
        };
        self.status.notify(notice, self.tick);
        vec![
            self.ctx
                .link
                .draft_import(seq, account, message, client_token()),
        ]
    }

    /// `d` on the pane: ask first.
    pub(super) fn confirm_discard_row(&mut self) -> Vec<Effect> {
        let Some(row) = self.drafts.pane.selected().cloned() else {
            self.status.notify("No draft to discard.", self.tick);
            return Vec::new();
        };
        let drafts = self
            .kind_of(row.account_id)
            .unwrap_or(AccountKind::Gmail)
            .drafts_name();
        let detail = match row.kind {
            DraftsRowKind::Local {
                on_remote: false, ..
            } => "It is kept only on this computer: it is gone for good.".to_string(),
            DraftsRowKind::Local {
                on_remote: true, ..
            } => format!("Its copy in {drafts} is deleted too."),
            DraftsRowKind::Remote { .. } => {
                if self.composing_busy() {
                    return Vec::new();
                }
                format!("It is deleted from {drafts}.")
            }
        };
        let lines = vec!["Discard this draft? (y/n)".to_string(), detail];
        self.drafts.discarding = Some(row);
        self.modal = Some(Modal::Confirm(Confirm::new(
            "Discard?",
            lines,
            Command::DiscardDraftRow,
        )));
        Vec::new()
    }

    /// The discard question answered yes: a local draft is discarded; a provider one is imported
    /// first, then its draft discarded.
    pub(super) fn discard_draft_row(&mut self) -> Vec<Effect> {
        let Some(row) = self.drafts.discarding.take() else {
            return Vec::new();
        };
        match row.kind {
            DraftsRowKind::Local { draft, .. } => vec![self.ctx.link.draft_discard(draft)],
            DraftsRowKind::Remote { .. } if self.composing_busy() => Vec::new(),
            DraftsRowKind::Remote { message } => {
                self.import_row(row.account_id, message, AfterImport::Discard)
            }
        }
    }

    /// The discard question closed without an answer.
    pub(super) fn discard_row_closed(&mut self) {
        self.drafts.discarding = None;
    }

    /// `draft.import` answered: the draft opens or is discarded; a failure is shown.
    pub(super) fn draft_imported(
        &mut self,
        seq: u64,
        result: Result<DraftImportResponse, OpError>,
    ) -> Vec<Effect> {
        let Some(Pending::Importing { seq: wanted, then }) = self.composing.pending else {
            return Vec::new();
        };
        if wanted != seq {
            return Vec::new();
        }
        self.composing.pending = None;
        let answer = match result {
            Ok(answer) => answer,
            Err(error) => {
                tracing::warn!(code = ?error.code, "draft.import failed");
                self.status.failed(&error);
                let what = match then {
                    AfterImport::Open => "open",
                    AfterImport::Discard => "discard",
                };
                let text = format!("Could not {what} the draft: {}", error_line(&error));
                self.status.error(text, self.ctx.clock.now(), self.tick);
                return Vec::new();
            }
        };
        self.status.answered();
        let mut effects = match then {
            AfterImport::Open => self.resume_draft(answer.draft),
            AfterImport::Discard => vec![self.ctx.link.draft_discard(answer.draft)],
        };
        effects.extend(self.drafts_changed());
        effects
    }

    /// A draft was made, changed or removed: the pane is read again while it shows.
    pub(super) fn drafts_changed(&mut self) -> Option<Effect> {
        self.in_drafts().then(|| self.read_drafts_pane())
    }

    /// A Drafts-folder read answered: the pane's, or `:drafts`'s.
    pub(super) fn drafts_folder_read(
        &mut self,
        seq: u64,
        result: Result<Vec<DraftsRow>, ReadError>,
    ) -> Vec<Effect> {
        if seq != self.drafts.latest {
            return self.drafts_picker_read(seq, result);
        }
        match result {
            Ok(rows) => self.drafts.pane.apply(rows),
            Err(error) => {
                self.side_read_failed(error, "drafts");
                self.drafts.pane.fail();
            }
        }
        Vec::new()
    }

    /// `:drafts`: the Drafts-folder rows of the scope's account (every account's in the
    /// unified inbox and the merged mailboxes).
    pub(super) fn drafts_picker(&mut self) -> Vec<Effect> {
        if self.composing_busy() {
            return Vec::new();
        }
        let seq = self.next_seq();
        self.composing.pending = Some(Pending::Drafts { seq });
        vec![self.ctx.store.drafts_folder(seq, self.scope_account_ids())]
    }

    fn drafts_picker_read(
        &mut self,
        seq: u64,
        result: Result<Vec<DraftsRow>, ReadError>,
    ) -> Vec<Effect> {
        if self.composing.pending != Some(Pending::Drafts { seq }) {
            return Vec::new();
        }
        self.composing.pending = None;
        match result {
            Ok(rows) if rows.is_empty() => self.status.notify("No drafts.", self.tick),
            Ok(rows) => {
                let items = rows
                    .iter()
                    .enumerate()
                    .map(|(i, row)| self.draft_item(i, row))
                    .collect();
                self.drafts.picked = rows;
                self.modal = Some(Modal::Picker(Picker::new(PickerKind::Draft, items)));
            }
            Err(error) => {
                self.side_read_failed(error, "drafts");
            }
        }
        Vec::new()
    }

    /// The `:drafts` answer: the row at `index` opens as Enter on the pane would.
    pub(super) fn picked_draft(&mut self, index: usize) -> Vec<Effect> {
        match self.drafts.picked.get(index).cloned() {
            Some(row) => self.open_draft_row(row),
            None => Vec::new(),
        }
    }

    /// `subject · to · when`, raw: the picker escapes names when it draws them. A draft kept
    /// only on this computer says `(local)` for its recipients.
    fn draft_item(&self, index: usize, row: &DraftsRow) -> PickerItem {
        let subject = if row.subject.trim().is_empty() {
            "(no subject)"
        } else {
            row.subject.as_str()
        };
        let to = match row.kind {
            DraftsRowKind::Local {
                on_remote: false, ..
            } => "(local)".to_string(),
            _ => row
                .to
                .iter()
                .map(|r| prefill::display_parts(r.name.as_deref(), &r.address))
                .collect::<Vec<_>>()
                .join(", "),
        };
        let when = self.ctx.clock.short(row.date);
        PickerItem {
            id: i64::try_from(index).unwrap_or(i64::MAX),
            name: format!("{subject} · {to} · {when}"),
            checked: false,
            swatch: Vec::new(),
        }
    }
}
