//! The mail screen's actions: the selection, the thread actions from the list and the
//! reader, the trash confirmation, the undo notice and `z`, mark-read on open, failures on
//! their rows and in `:errors`. Every call goes through the daemon link; the store change
//! comes back as a re-read; the calls' answers are `answers`'.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::time::Instant;

use rail_core::ops::{
    Action as MailAction, ActionFailed, MAX_ACTION_THREADS, OpError, RowToken, client_token,
};
use rail_core::store::view::ThreadDetail;

use super::selection::{Picked, Selection};
use super::{Focus, MailScreen, error_line};
use crate::tui::app::Effect;
use crate::tui::data::{ApplyCall, MailEvent};
use crate::tui::keys::Action;
use crate::tui::modals::Command;

mod answers;

/// Ticks the undo notice stays: `UNDO_GRACE` (10 s) at one tick per 250 ms.
pub const UNDO_TICKS: usize = 40;

/// The newest action with grace, while `z` can take it back.
pub(super) struct LastAction {
    seq: u64,
    /// The rows the daemon queued, each with the token it was queued under.
    rows: Vec<RowToken>,
    count: usize,
    until: usize,
    /// When `UNDO_GRACE` of wall time has passed: ticks are skipped while the terminal is
    /// suspended, so `until` alone could offer `z` long after the rows were sent.
    deadline: Instant,
    /// `action.apply` calls (one per account per `MAX_ACTION_THREADS` threads) not answered yet.
    pending: usize,
    /// `z` was pressed: rows are undone as their calls answer.
    undo_wanted: bool,
    /// `action.undo` calls not answered yet.
    undos: usize,
    /// Some undo answered `too_late`.
    too_late: bool,
    /// Some undo took rows back. Over a bulk action's chunks an answer can hold both, and
    /// `too late to undo` would then report the opposite of what mostly happened.
    some_undone: bool,
    /// The targets that came from the selection, put back if a call fails.
    selected: Vec<(i64, Picked)>,
    /// Some failed call already put its targets back.
    restored: bool,
    /// Some call failed: its error stays on the bar while the others answer.
    failed: bool,
    /// Some undo was answered.
    undid: bool,
    /// Some undo failed: its error stays on the bar.
    undo_failed: bool,
    /// What the record is: undoing a send reopens its review.
    kind: Record,
    /// What a snooze's answered calls snoozed, and left out (no mail in the inbox).
    snoozed: usize,
    left_out: usize,
    /// The threads of a snooze's calls refused as not in the inbox, and the refusal: said, and
    /// the threads put back, only if the snooze took nothing.
    refused: Vec<i64>,
    refusal: Option<OpError>,
}

/// What an undo record took: thread actions, a `draft.send` or a `draft.push`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Record {
    Threads,
    Sent,
    Pushed,
}

/// Counts and flags only: the record's thread ids and rows are not named.
impl fmt::Debug for LastAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LastAction")
            .field("seq", &self.seq)
            .field("rows", &self.rows.len())
            .field("count", &self.count)
            .field("pending", &self.pending)
            .field("undo_wanted", &self.undo_wanted)
            .field("undos", &self.undos)
            .field("undo_failed", &self.undo_failed)
            .field("selected", &self.selected.len())
            .field("failed", &self.failed)
            .field("kind", &self.kind)
            .field("snoozed", &self.snoozed)
            .field("left_out", &self.left_out)
            .field("refused", &self.refused.len())
            .finish_non_exhaustive()
    }
}

impl LastAction {
    fn new(seq: u64, count: usize, until: usize, selected: Vec<(i64, Picked)>) -> Self {
        Self {
            seq,
            rows: Vec::new(),
            count,
            until,
            deadline: Instant::now() + rail_core::ops::UNDO_GRACE,
            pending: 0,
            undo_wanted: false,
            undos: 0,
            too_late: false,
            some_undone: false,
            selected,
            restored: false,
            failed: false,
            undid: false,
            undo_failed: false,
            kind: Record::Threads,
            snoozed: 0,
            left_out: 0,
            refused: Vec::new(),
            refusal: None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ActionState {
    pub(crate) selection: Selection,
    last: Option<LastAction>,
    /// Records a newer action replaced while some of their calls were unanswered, by seq:
    /// their answers still put a failed call's selection back and honour a `z` pressed for
    /// them, though only `last` is undoable now.
    pub(super) replaced: BTreeMap<u64, LastAction>,
    seq: u64,
    /// Threads whose action failed: the row and the escaped line on their list row.
    pub(crate) failures: BTreeMap<i64, (RowToken, String)>,
    /// What `y` on a thread-action confirmation applies: the action and its targets.
    pub(super) confirming: Option<(MailAction, Vec<(i64, Picked)>)>,
    /// The picker waiting for its labels, then open: what its answer applies to.
    pub(super) picking: Option<super::actions_pick::Picking>,
    /// Each "Return to inbox now" while its calls answer, by seq: the newest says the count, and
    /// a failed call of any gives its threads back.
    pub(super) returning: BTreeMap<u64, super::snooze_flow::Returning>,
    /// The seq of the newest "Return to inbox now" sent: only its answers say the count.
    pub(super) newest_return: u64,
    /// The last time typed into the snooze chooser, offered again as "Last used".
    pub(super) last_snooze: Option<String>,
}

/// "archived" and "archive", for the notice and the failure line.
pub(crate) fn verbs(action: &MailAction) -> (&'static str, &'static str) {
    match action {
        MailAction::Archive {} => ("archived", "archive"),
        MailAction::Trash {} => ("trashed", "trash"),
        MailAction::Star { on: true } => ("starred", "star"),
        MailAction::Star { on: false } => ("unstarred", "unstar"),
        MailAction::Read { on: true } => ("marked read", "mark read"),
        MailAction::Read { on: false } => ("marked unread", "mark unread"),
        MailAction::Labels { .. } => ("labelled", "change the labels of"),
        MailAction::Move { .. } => ("moved", "move"),
        MailAction::Send { .. } => ("sent", "send"),
        MailAction::PushDraft { .. } => ("saved", "save the draft"),
        MailAction::DeleteDraft { .. } => ("discarded", "discard the draft"),
        MailAction::Unsnooze { .. } => ("returned to the inbox", "return to the inbox"),
    }
}

/// After a failed call, the threads of `threads` among `selected` come back to the selection,
/// if nothing was selected since (or an earlier failed call already put its threads back:
/// `restored`).
pub(super) fn put_back_threads(
    selected: &[(i64, Picked)],
    restored: &mut bool,
    selection: &mut Selection,
    threads: &[i64],
) {
    if *restored || selection.is_empty() {
        *restored = true;
        let threads: HashSet<i64> = threads.iter().copied().collect();
        let back: Vec<(i64, Picked)> = selected
            .iter()
            .filter(|(id, _)| threads.contains(id))
            .copied()
            .collect();
        selection.restore(&back);
    }
}

impl MailScreen {
    /// The action keys; `None` for every other action.
    pub(super) fn mail_action(&mut self, action: Action) -> Option<Vec<Effect>> {
        let thread_view = matches!(self.focus, Focus::List | Focus::Reader);
        Some(match action {
            Action::GlobalUndo => self.undo(),
            Action::ListSelect if self.focus == Focus::List => self.toggle_selected(),
            Action::ListClearSelection if self.focus == Focus::List => {
                self.clear_selection_or_range()
            }
            Action::ListSelectAll if self.focus == Focus::List => self.select_all(),
            Action::ListSelectSender if self.focus == Focus::List => self.select_sender(),
            Action::ListSelectRange if self.focus == Focus::List => self.select_range(),
            Action::ListExtendDown if self.focus == Focus::List => self.extend_selection(true),
            Action::ListExtendUp if self.focus == Focus::List => self.extend_selection(false),
            Action::ListOpenLink if thread_view => self.open_subject_link(),
            Action::ListArchive
            | Action::ListTrash
            | Action::ListTrashNow
            | Action::ListStar
            | Action::ListRead
            | Action::ListLabels
            | Action::ListMove
            | Action::ListSnooze
                if thread_view =>
            {
                self.thread_action(action)
            }
            _ => return None,
        })
    }

    fn toggle_selected(&mut self) -> Vec<Effect> {
        if let Some(row) = self.list.selected().cloned() {
            self.actions.selection.toggle(&row);
            // `(more match)` belongs to the capped select-all that set it.
            self.bulk.more = false;
        }
        self.list_action(Action::ListDown)
    }

    /// The reader's thread; else the selection; else the cursor row.
    pub(super) fn targets(&self) -> Vec<(i64, Picked)> {
        if self.focus == Focus::Reader {
            return self
                .reader
                .as_ref()
                .map(|r| {
                    let d = r.detail();
                    let picked = Picked {
                        account_id: d.account_id,
                        starred: d.messages.iter().any(|m| m.is_starred),
                        unread: d.messages.iter().any(|m| !m.is_read),
                    };
                    vec![(d.thread_id, picked)]
                })
                .unwrap_or_default();
        }
        if !self.actions.selection.is_empty() {
            return self.actions.selection.targets();
        }
        self.list
            .selected()
            .map(|r| vec![(r.key.thread_id, Picked::of(r))])
            .unwrap_or_default()
    }

    fn thread_action(&mut self, action: Action) -> Vec<Effect> {
        let targets = self.targets();
        if targets.is_empty() {
            self.status.notify("No thread to act on.", self.tick);
            return Vec::new();
        }
        if matches!(action, Action::ListLabels) && self.refuse_m365_labels(&targets) {
            return Vec::new();
        }
        let mail = match action {
            Action::ListArchive => MailAction::Archive {},
            Action::ListTrash => return self.confirm_trash(targets),
            // As `d` without its question; a large selection is still asked about below.
            Action::ListTrashNow => MailAction::Trash {},
            Action::ListStar => MailAction::Star {
                on: !targets.iter().all(|(_, p)| p.starred),
            },
            Action::ListRead => MailAction::Read {
                on: targets.iter().any(|(_, p)| p.unread),
            },
            Action::ListLabels => {
                return self.open_picker(crate::tui::modals::PickerKind::Labels, targets);
            }
            Action::ListMove => {
                return self.open_picker(crate::tui::modals::PickerKind::Move, targets);
            }
            Action::ListSnooze => return self.open_snooze(targets),
            _ => return Vec::new(),
        };
        // A large set is asked about first; the picker's own `Enter` is `l`/`m`'s confirmation.
        if super::bulk::needs_confirm(&mail, targets.len(), self.ctx.bulk_confirm) {
            return self.confirm_action(mail, targets);
        }
        self.send_action(mail, &targets, true)
    }

    pub(super) fn account_name(&self, account_id: i64) -> Option<String> {
        let index = self
            .account_ids
            .iter()
            .position(|id| *id == Some(account_id))?;
        self.accounts.get(index).map(|a| a.name.clone())
    }

    /// Send `action` for `targets`, one call per account; with grace it becomes the undoable
    /// action. Clears the selection; archive, trash and move close the reader.
    pub(super) fn send_action(
        &mut self,
        action: MailAction,
        targets: &[(i64, Picked)],
        grace: bool,
    ) -> Vec<Effect> {
        self.send_with(&action, targets, grace, None)
    }

    /// `send_action`; with `snooze`, a `snooze.set` until then whose archive is `action`.
    pub(super) fn send_with(
        &mut self,
        action: &MailAction,
        targets: &[(i64, Picked)],
        grace: bool,
        snooze: Option<i64>,
    ) -> Vec<Effect> {
        let by_account = self.group_by_account(targets);
        self.actions.seq += 1;
        let seq = self.actions.seq;
        if grace {
            let from_selection = self.targets_are_selection(targets);
            let selected = if from_selection {
                targets.to_vec()
            } else {
                Vec::new()
            };
            let until = self.tick + UNDO_TICKS;
            self.replace_last(LastAction::new(seq, targets.len(), until, selected));
            if from_selection {
                self.actions.selection.clear();
                self.bulk.more = false;
            }
        }
        let leaves = matches!(
            action,
            MailAction::Archive {} | MailAction::Trash {} | MailAction::Move { .. }
        );
        let closed = if grace && leaves && self.focus == Focus::Reader {
            self.close_reader()
        } else {
            Vec::new()
        };
        let effects = self.dispatch(seq, by_account, action, (grace, snooze));
        if grace {
            if effects.is_empty() {
                self.actions.last = None;
            } else if let Some(last) = self.actions.last.as_mut() {
                last.pending = effects.len();
            }
        }
        closed.into_iter().chain(effects).collect()
    }

    /// `next` becomes the undo record; the one it replaces is kept while calls are unanswered.
    fn replace_last(&mut self, next: LastAction) {
        if let Some(old) = self
            .actions
            .last
            .take()
            .filter(|l| l.pending > 0 || l.undos > 0)
        {
            self.actions.replaced.insert(old.seq, old);
        }
        self.actions.last = Some(next);
    }

    /// The targets' thread ids by account; their old failure lines go.
    pub(super) fn group_by_account(
        &mut self,
        targets: &[(i64, Picked)],
    ) -> BTreeMap<i64, Vec<i64>> {
        let mut by_account: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for (thread, picked) in targets {
            by_account
                .entry(picked.account_id)
                .or_default()
                .push(*thread);
            self.actions.failures.remove(thread);
        }
        by_account
    }

    /// One `action.apply` (a `snooze.set` with a snooze's end) per account per
    /// `MAX_ACTION_THREADS` threads, in thread-id order, each under its own client token so a
    /// failed chunk is retried on its own.
    fn dispatch(
        &mut self,
        seq: u64,
        by_account: BTreeMap<i64, Vec<i64>>,
        action: &MailAction,
        (grace, snooze): (bool, Option<i64>),
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        for (account_id, threads) in by_account {
            let Some(account) = self.account_name(account_id) else {
                self.status
                    .notify("This account is no longer configured.", self.tick);
                continue;
            };
            for chunk in threads.chunks(MAX_ACTION_THREADS) {
                effects.push(self.ctx.link.apply(ApplyCall {
                    seq,
                    account: account.clone(),
                    threads: chunk.to_vec(),
                    action: action.clone(),
                    grace,
                    client_token: client_token(),
                    snooze,
                }));
            }
        }
        effects
    }

    /// The targets are the list's selection (not the open thread or the cursor row).
    pub(super) fn targets_are_selection(&self, targets: &[(i64, Picked)]) -> bool {
        let selected = self.actions.selection.targets();
        self.focus != Focus::Reader
            && !selected.is_empty()
            && selected
                .iter()
                .map(|(id, _)| id)
                .eq(targets.iter().map(|(id, _)| id))
    }

    /// `z`: take back the newest action. Calls still unanswered are undone as they answer.
    fn undo(&mut self) -> Vec<Effect> {
        let tick = self.tick;
        let Some(last) = self.actions.last.as_mut() else {
            self.status.notify("Nothing to undo.", tick);
            return Vec::new();
        };
        if last.undo_wanted {
            return Vec::new();
        }
        let late = Instant::now() >= last.deadline;
        if late || (last.pending == 0 && tick >= last.until) {
            self.actions.last = None;
            self.status.notify("Too late to undo.", tick);
            return Vec::new();
        }
        last.undo_wanted = true;
        // Threads refused as not in the inbox were never snoozed: they come back now, as a
        // failed call's do.
        let refused = std::mem::take(&mut last.refused);
        if !refused.is_empty() {
            let selection = &mut self.actions.selection;
            put_back_threads(&last.selected, &mut last.restored, selection, &refused);
        }
        let rows = std::mem::take(&mut last.rows);
        let waiting = last.pending > 0;
        if rows.is_empty() {
            if waiting {
                self.status.notify("undoing…", tick);
            } else {
                self.actions.last = None;
                self.status.notify("Nothing to undo.", tick);
            }
            return Vec::new();
        }
        let batches = super::bulk::undo_batches(&rows);
        last.undos += batches.len();
        let seq = last.seq;
        if waiting {
            self.status.notify("undoing…", tick);
        }
        batches
            .into_iter()
            .map(|batch| self.ctx.link.undo(seq, batch))
            .collect()
    }

    /// Mark-read on open: no grace, no notice, nothing to undo.
    /// The detail is patched to read at once, so the reader and its keys agree with what was
    /// sent before the store's change comes back.
    pub(super) fn mark_read_on_open(&mut self, detail: &mut ThreadDetail) -> Vec<Effect> {
        if detail.messages.iter().all(|m| m.is_read) {
            return Vec::new();
        }
        for message in &mut detail.messages {
            message.is_read = true;
        }
        let picked = Picked {
            account_id: detail.account_id,
            starred: false,
            unread: true,
        };
        self.send_action(
            MailAction::Read { on: true },
            &[(detail.thread_id, picked)],
            false,
        )
    }

    pub(super) fn action_command(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Trash => {
                let Some((action, targets)) = self.actions.confirming.take() else {
                    return Vec::new();
                };
                self.send_action(action, &targets, true)
            }
            Command::Retry(row) => {
                self.status.retried(row.row);
                self.actions.failures.retain(|_, (r, _)| r.row != row.row);
                vec![self.ctx.link.retry(row)]
            }
            Command::SaveSearch { name } => self.save_search(name),
            Command::RenameSearch { index, old, name } => self.rename_search(index, &old, name),
            Command::RemoveSearch { index, name } => self.remove_search(index, &name),
            other => match self.compose_command(other) {
                Ok(effects) => effects,
                Err(other) => self.picker_command(other),
            },
        }
    }

    pub(super) fn action_event(&mut self, event: MailEvent) -> Vec<Effect> {
        match event {
            MailEvent::Applied { call, result } => self.applied(&call, result),
            MailEvent::Undone { seq, result } => self.undo_answered(seq, result),
            MailEvent::Retried { result, .. } => {
                match result {
                    Ok(answer) if answer.retried => {
                        self.status.notify("sending again", self.tick);
                    }
                    Ok(_) => self
                        .status
                        .notify("That action is no longer waiting for a retry.", self.tick),
                    Err(error) => self.action_error("retry", &error),
                }
                self.dirty()
            }
            MailEvent::ActionFailed(failure) => self.action_failed(*failure),
            MailEvent::Matched { seq, result } => self.matched(seq, result),
            MailEvent::Sender { seq, result } => self.sender_read(seq, result),
            MailEvent::Labels { seq, result } => self.labels_read(seq, result),
            MailEvent::ThemeSaved { result } => self.setting_saved("theme", result),
            MailEvent::KeymapSaved { result } => self.setting_saved("key preset", result),
            MailEvent::HintsSaved { result } => self.setting_saved("hints setting", result),
            MailEvent::ClockSaved { result } => self.setting_saved("clock setting", result),
            MailEvent::SearchCountsSaved { result } => {
                self.setting_saved("saved-search counts setting", result)
            }
            MailEvent::RichHtmlSaved { result } => self.setting_saved("HTML mail setting", result),
            MailEvent::ImagesSaved { result } => self.setting_saved("images setting", result),
            MailEvent::RemoteImagesSaved { result } => {
                self.setting_saved("remote images setting", result)
            }
            MailEvent::TimeSaved { result } => self.setting_saved("time setting", result),
            MailEvent::ThreadOrderSaved { result } => self.setting_saved("thread order", result),
            MailEvent::DimReadSaved { result } => {
                self.setting_saved("read dimming setting", result)
            }
            other => self.compose_event(other),
        }
    }

    /// `draft.send` queued the draft: `z` takes it back while the grace lasts.
    pub(super) fn sent_record(&mut self, row: RowToken) {
        self.draft_record(row, Record::Sent, "sent");
    }

    /// `draft.push` queued the draft for `provider`'s Drafts under `row`'s token: `z` takes it
    /// back while the grace lasts, and the draft stays on this computer.
    pub(super) fn pushed_record(&mut self, row: RowToken, provider: &str) {
        self.draft_record(row, Record::Pushed, &format!("saved to {provider}"));
    }

    fn draft_record(&mut self, row: RowToken, kind: Record, done: &str) {
        self.actions.seq += 1;
        let until = self.tick + UNDO_TICKS;
        let mut last = LastAction::new(self.actions.seq, 1, until, Vec::new());
        last.rows.push(row);
        last.kind = kind;
        self.replace_last(last);
        let key = self.ctx.keymap.label(Action::GlobalUndo);
        self.status
            .notify_for(format!("{done} · {key} undoes"), self.tick, UNDO_TICKS);
    }

    pub(super) fn action_error(&mut self, what: &str, error: &OpError) {
        tracing::warn!(code = ?error.code, "an action request failed");
        let text = format!("Could not {what}: {}", error_line(error));
        self.status.error(text, self.ctx.clock.now(), self.tick);
    }

    fn action_failed(&mut self, failure: ActionFailed) -> Vec<Effect> {
        let what = match &failure.action {
            MailAction::PushDraft { .. } => format!(
                "{} to {}",
                verbs(&failure.action).1,
                self.kind_named(&failure.account).provider_name()
            ),
            action => verbs(action).1.to_string(),
        };
        let text = format!("Could not {what}: {}", error_line(&failure.error));
        let row = RowToken {
            row: failure.row,
            client_token: failure.client_token.clone(),
        };
        for thread in &failure.threads {
            self.actions
                .failures
                .insert(*thread, (row.clone(), text.clone()));
        }
        // The daemon sends the failed rows again on every connect: a row still offered for a
        // retry is listed once.
        let mut effects = Vec::new();
        if !self.status.offers_retry(failure.row) {
            self.status
                .error_with_retry(text, self.ctx.clock.now(), self.tick, row);
            if let (MailAction::Send { .. }, Some(draft)) = (&failure.action, failure.draft) {
                effects = self.send_failed(draft, &failure.error);
            }
        }
        effects.extend(self.dirty());
        effects
    }
}

#[cfg(test)]
impl MailScreen {
    /// As if the terminal was suspended past the undo window: the ticks did not move.
    pub(super) fn pass_undo_deadline(&mut self) {
        if let Some(last) = self.actions.last.as_mut() {
            last.deadline = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_undo_window_is_the_grace() {
        assert_eq!(
            UNDO_TICKS as u128 * crate::tui::run::TICK.as_millis(),
            rail_core::ops::UNDO_GRACE.as_millis()
        );
    }
}
