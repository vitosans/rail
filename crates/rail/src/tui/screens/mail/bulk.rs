//! Bulk selection: `* a` selects every thread the mailbox and filter match, `* f` everything
//! from one sender, `v` a range, `J`/`K` (Shift-↓/↑) extend it a row at a time; the state those
//! keys keep between presses, and the question a large archive, star or mark-read asks first.

use std::collections::BTreeSet;

use rail_core::ops::{Action as MailAction, MAX_UNDO_ROWS, RowToken};
use rail_core::provider::Mailbox;
use rail_core::store::filter::Filter;
use rail_core::store::view::ThreadKey;

use super::header::thousands;
use super::selection::Picked;
use super::{MailScreen, Modal};
use crate::tui::app::Effect;
use crate::tui::data::{ReadError, ScopeChoice};
use crate::tui::keys::Action;
use crate::tui::modals::{Command, Confirm};
use crate::tui::text::{escape, fit};

/// Characters of one thread's subject that a question or notice shows: the trash question's,
/// and "Back from snooze".
pub(super) const SHOWN_SUBJECT_CHARS: usize = 120;

/// Threads one bulk select takes at most: ~50 `action.apply` chunks and ~1 undo chunk per
/// account, about 300 KB of ids and flags.
pub(crate) const MAX_SELECTION: usize = 10_000;

/// What the bulk keys keep between presses.
#[derive(Debug, Default)]
pub(crate) struct BulkState {
    /// `v`: the thread the range anchors on, while range mode is on.
    anchor: Option<i64>,
    /// The last select-all was capped: the header says `(more match)`.
    pub(super) more: bool,
    /// The newest `* a` read's sequence number: an older answer is dropped.
    pub(super) latest_match: u64,
    /// The mailbox and filter the newest `* a` read was asked for.
    pub(super) match_shown: Option<Shown>,
    /// The newest `* f` read's sequence number.
    pub(super) latest_sender: u64,
    /// The mailbox and filter the newest `* f` read was asked for.
    pub(super) sender_shown: Option<Shown>,
    /// The Shift range under way: any key but `J`/`K` and Shift-↓/↑ ends it.
    pub(super) extending: Option<Extending>,
}

/// A Shift range (`J`/`K`, Shift-↓/↑): the thread it anchors on, and the threads it selected
/// that were not selected before it began — only those leave when it shrinks.
#[derive(Debug)]
pub(super) struct Extending {
    anchor: i64,
    added: BTreeSet<i64>,
}

/// The mailbox and filter the list shows, recorded when a `* a` or `* f` read is asked for.
/// Its answer is applied only while the list still shows the same: compared on the answer, not
/// cleared where the scope or filter changes, so a new way to change them cannot miss it.
#[derive(Debug)]
pub(super) struct Shown {
    scope: ScopeChoice,
    filter: Filter,
}

impl BulkState {
    /// Range mode is on.
    pub(super) fn range_on(&self) -> bool {
        self.anchor.is_some()
    }
}

/// The confirmation's box title for `action`. Fixed words: no mail content reaches a question.
pub(super) fn title(action: &MailAction) -> &'static str {
    match action {
        MailAction::Archive {} => "Archive?",
        MailAction::Trash {} => "Trash?",
        MailAction::Star { on: true } => "Star?",
        MailAction::Star { on: false } => "Unstar?",
        MailAction::Read { on: true } => "Mark read?",
        MailAction::Read { on: false } => "Mark unread?",
        MailAction::Labels { .. } => "Change the labels?",
        MailAction::Move { .. } => "Move?",
        MailAction::Send { .. } => "Send?",
        MailAction::PushDraft { .. } => "Save the draft?",
        MailAction::DeleteDraft { .. } => "Discard the draft?",
        MailAction::Unsnooze { .. } => "Return to the inbox?",
    }
}

/// The confirmation's question for `action` over `count` threads, counted with commas. Fixed
/// words and a number only, so no mail content can reach it.
pub(super) fn question(action: &MailAction, count: usize) -> String {
    let n = thousands(u64::try_from(count).unwrap_or(u64::MAX));
    match action {
        MailAction::Archive {} => format!("Archive {n} threads?"),
        MailAction::Trash {} => format!("Trash {n} threads?"),
        MailAction::Star { on: true } => format!("Star {n} threads?"),
        MailAction::Star { on: false } => format!("Unstar {n} threads?"),
        MailAction::Read { on: true } => format!("Mark {n} threads read?"),
        MailAction::Read { on: false } => format!("Mark {n} threads unread?"),
        MailAction::Labels { .. } => format!("Change the labels of {n} threads?"),
        MailAction::Move { .. } => format!("Move {n} threads?"),
        MailAction::Send { .. } => format!("Send {n} messages?"),
        MailAction::PushDraft { .. } => format!("Save {n} drafts?"),
        MailAction::DeleteDraft { .. } => format!("Discard {n} drafts?"),
        MailAction::Unsnooze { .. } => format!("Return {n} threads to the inbox?"),
    }
}

/// `action` over `count` threads asks first: more than `threshold` threads, which `0` turns
/// off. Only `e`, `s` and `u` ask this way — `d` asks at every size through its own question,
/// trash without asking applies the same threshold itself (`trash_now`), and the label and move
/// pickers' own `Enter` is their confirmation.
pub(super) fn needs_confirm(action: &MailAction, count: usize, threshold: u32) -> bool {
    let asks = matches!(
        action,
        MailAction::Archive {} | MailAction::Star { .. } | MailAction::Read { .. }
    );
    asks && threshold > 0 && u64::try_from(count).unwrap_or(u64::MAX) > u64::from(threshold)
}

/// `rows` split into the batches one `action.undo` takes. A bulk action enqueues one outbox
/// row per `action.apply` call — about 58 at `MAX_SELECTION` — so this is defensive; the
/// request's own bound is `MAX_UNDO_ROWS`, and one `z` must take the whole set back.
pub(super) fn undo_batches(rows: &[RowToken]) -> Vec<Vec<RowToken>> {
    rows.chunks(MAX_UNDO_ROWS)
        .map(<[RowToken]>::to_vec)
        .collect()
}

impl MailScreen {
    /// The mailbox and filter the list shows now.
    pub(super) fn shown(&self) -> Shown {
        Shown {
            scope: self.scope.clone(),
            filter: self.filter.clone(),
        }
    }

    /// The list still shows what a read recorded in `asked` was asked for.
    fn still_shown(&self, asked: Option<&Shown>) -> bool {
        asked.is_some_and(|shown| shown.scope == self.scope && shown.filter == self.filter)
    }

    /// `d`: the trash question, which asks at every size. One thread names its subject;
    /// several are counted, and each provider's note follows.
    pub(super) fn confirm_trash(&mut self, targets: Vec<(i64, Picked)>) -> Vec<Effect> {
        let first = targets.first().map(|(id, _)| *id);
        let subject = self
            .list
            .rows()
            .iter()
            .find(|r| Some(r.key.thread_id) == first)
            .map(|r| r.subject.clone())
            .or_else(|| self.reader.as_ref().map(|r| r.detail().subject.clone()));
        let line = match (targets.len(), subject) {
            (1, Some(subject)) => {
                format!("Trash \"{}\"?", fit(&escape(&subject), SHOWN_SUBJECT_CHARS))
            }
            (n, _) => super::bulk::question(&MailAction::Trash {}, n),
        };
        let mut lines = vec![line];
        lines.extend(self.trash_notes(&targets));
        self.actions.confirming = Some((MailAction::Trash {}, targets));
        self.modal = Some(Modal::Confirm(Confirm::new(
            "Trash?",
            lines,
            Command::Trash,
        )));
        Vec::new()
    }

    /// Ask before applying `action` to this many threads: the fixed question and count of
    /// `bulk`, over the list.
    pub(super) fn confirm_action(
        &mut self,
        action: MailAction,
        targets: Vec<(i64, Picked)>,
    ) -> Vec<Effect> {
        let lines = vec![super::bulk::question(&action, targets.len())];
        let head = super::bulk::title(&action);
        self.actions.confirming = Some((action, targets));
        self.modal = Some(Modal::Confirm(Confirm::new(head, lines, Command::Trash)));
        Vec::new()
    }

    /// `Esc` in the list: end range mode, keeping the range; else clear the selection
    /// (the filter line, when open, took the key before this).
    pub(super) fn clear_selection_or_range(&mut self) -> Vec<Effect> {
        if self.bulk.range_on() {
            return self.end_range();
        }
        self.actions.selection.clear();
        self.bulk.more = false;
        Vec::new()
    }

    /// `v`: anchor a range on the cursor thread, or end a mode already on. The lasting cue is
    /// the `Esc range` hint the bar draws first while the mode lasts; the notice expires like
    /// every other one.
    pub(super) fn select_range(&mut self) -> Vec<Effect> {
        if self.bulk.range_on() {
            return self.end_range();
        }
        let Some(thread_id) = self.list.selected().map(|row| row.key.thread_id) else {
            return Vec::new();
        };
        self.bulk.anchor = Some(thread_id);
        self.status.notify("range from here", self.tick);
        Vec::new()
    }

    /// End range mode, keeping the rows from the anchor to the cursor inclusive. The anchor is
    /// a thread id, so a re-read that drops it out of the window leaves nothing to measure
    /// from: the mode ends, nothing is added, and the bar says so.
    fn end_range(&mut self) -> Vec<Effect> {
        let anchor = self.bulk.anchor.take();
        let at = anchor.and_then(|id| {
            self.list
                .rows()
                .iter()
                .position(|row| row.key.thread_id == id)
        });
        let Some(at) = at else {
            self.status
                .notify("The range start is no longer in the list.", self.tick);
            return Vec::new();
        };
        let cursor = self.list.selected_index();
        let (first, last) = (at.min(cursor), at.max(cursor));
        let targets: Vec<(i64, Picked)> = self.list.rows()[first..=last]
            .iter()
            .map(|row| (row.key.thread_id, Picked::of(row)))
            .collect();
        let full = self.actions.selection.add(&targets);
        // `(more match)` belongs to the capped select-all that set it.
        self.bulk.more = false;
        if full {
            self.say_full();
        }
        Vec::new()
    }

    /// A range ran into the cap: the rows past it were left out, and the bar says why.
    fn say_full(&mut self) {
        let cap = thousands(u64::try_from(MAX_SELECTION).unwrap_or(u64::MAX));
        let text = format!("The selection holds at most {cap} threads.");
        self.status.notify(text, self.tick);
    }

    /// `* a`: read every thread this mailbox and filter match, up to one over the cap, so the
    /// answer says whether more match. One read per press, the older one dropped by its `seq`.
    pub(super) fn select_all(&mut self) -> Vec<Effect> {
        let request = self.match_request();
        self.status.notify("selecting…", self.tick);
        vec![self.ctx.store.matching_targets(request, MAX_SELECTION + 1)]
    }

    /// `Store::matching_targets` answered: add the newest `MAX_SELECTION` to the selection and
    /// say so, or report the read's error. An answer a newer press superseded, or one for a
    /// mailbox or filter the list no longer shows, is dropped: it would select threads that
    /// are not on screen.
    pub(super) fn matched(
        &mut self,
        seq: u64,
        result: Result<Vec<(ThreadKey, i64, bool, bool)>, ReadError>,
    ) -> Vec<Effect> {
        if seq != self.bulk.latest_match || !self.still_shown(self.bulk.match_shown.as_ref()) {
            return Vec::new();
        }
        let answer = match result {
            Ok(answer) => answer,
            // `Stale` and a store that is not set up say nothing here: a newer press is on its
            // way, and the list already says the store is not ready. A failure names no thread.
            Err(error) => {
                self.side_read_failed(error, "selection");
                return Vec::new();
            }
        };
        if answer.is_empty() {
            self.status.notify("Nothing matches.", self.tick);
            return Vec::new();
        }
        let over_the_cap = answer.len() > MAX_SELECTION;
        let targets: Vec<(i64, Picked)> = answer
            .into_iter()
            .map(|(key, account_id, starred, unread)| {
                (
                    key.thread_id,
                    Picked {
                        account_id,
                        starred,
                        unread,
                    },
                )
            })
            .collect();
        let truncated = self.actions.selection.add(&targets);
        self.bulk.more = over_the_cap || truncated;
        let count = thousands(u64::try_from(self.actions.selection.len()).unwrap_or(u64::MAX));
        self.status.notify(format!("{count} selected"), self.tick);
        // A selection change is not a store change: no re-read, so the flags the read
        // answered are the flags the selection keeps.
        Vec::new()
    }

    /// `* f`: read the `From` of the cursor thread's newest message someone other than the user
    /// sent, to filter by.
    pub(super) fn select_sender(&mut self) -> Vec<Effect> {
        let Some(thread_id) = self.list.selected().map(|row| row.key.thread_id) else {
            return Vec::new();
        };
        let seq = self.next_seq();
        self.bulk.latest_sender = seq;
        self.bulk.sender_shown = Some(self.shown());
        vec![self.ctx.store.sender(seq, thread_id)]
    }

    /// `Store::newest_sender` answered: put the address in the filter line as the exact form
    /// and select everything it matches. An answer a newer press superseded, or one for a
    /// mailbox or filter the list no longer shows, is dropped.
    pub(super) fn sender_read(
        &mut self,
        seq: u64,
        result: Result<Option<Mailbox>, ReadError>,
    ) -> Vec<Effect> {
        if seq != self.bulk.latest_sender || !self.still_shown(self.bulk.sender_shown.as_ref()) {
            return Vec::new();
        }
        let sender = match result {
            Ok(sender) => sender,
            Err(error) => {
                self.side_read_failed(error, "sender");
                return Vec::new();
            }
        };
        // No address, or one the filter grammar cannot carry: the filter and the selection are
        // left alone and no matching read is sent.
        let Some(text) = sender.and_then(|mailbox| filter_term(&mailbox.address)) else {
            self.status
                .notify("This thread has no sender to select by.", self.tick);
            return Vec::new();
        };
        self.filter_text = text;
        let mut effects = self.apply_filter();
        effects.extend(self.select_all());
        effects
    }
}

/// `from:<address>` when `address` survives the round trip through the filter grammar: the
/// text has to parse back to exactly that one `from_exact` term and nothing else.
///
/// The address is sender-controlled input to a mini-language. `tokens()` splits at white space
/// and toggles on every `"`, so `"dana reyes"@example.com` would parse as `from:dana` plus a
/// word; and an empty value turns `from:` into a word, which `fts_query` then runs as a phrase
/// over every message's subject, sender, recipients and body — a large part of the mailbox,
/// one key from being archived. Checking the round trip is what rules all of that out.
fn filter_term(address: &str) -> Option<String> {
    let text = format!("from:<{address}>");
    let wanted = Filter {
        from_exact: vec![address.to_string()],
        ..Filter::default()
    };
    (rail_core::store::filter::parse(&text) == wanted).then_some(text)
}

impl MailScreen {
    /// `J`/`K` (Shift-↓/↑): the first press anchors on the cursor row; each press moves the
    /// cursor one row and selects every row from the anchor to it, and moving back toward the
    /// anchor deselects the rows this range added. In `v` range mode the keys only move.
    pub(super) fn extend_selection(&mut self, down: bool) -> Vec<Effect> {
        let step = if down {
            Action::ListDown
        } else {
            Action::ListUp
        };
        if self.bulk.range_on() {
            return self.list_action(step);
        }
        let Some(cursor) = self.list.selected().map(|row| row.key.thread_id) else {
            return Vec::new();
        };
        let anchor = self
            .bulk
            .extending
            .get_or_insert_with(|| Extending {
                anchor: cursor,
                added: BTreeSet::new(),
            })
            .anchor;
        let effects = self.list_action(step);
        self.extend_from(anchor, cursor);
        effects
    }

    /// The rows from `anchor` to the cursor become the Shift range's selection. A sync's re-read
    /// starts at the screen's first row, so a long range can lose its anchor: it then anchors
    /// afresh on `from`, the row the cursor just left, and only the older rows' shrink is lost.
    fn extend_from(&mut self, anchor: i64, from: i64) {
        let rows = self.list.rows();
        let find = |id: i64| rows.iter().position(|row| row.key.thread_id == id);
        let at = if let Some(at) = find(anchor) {
            at
        } else {
            let Some(at) = find(from) else {
                self.bulk.extending = None;
                return;
            };
            if let Some(extending) = self.bulk.extending.as_mut() {
                extending.anchor = from;
                extending.added.clear();
            }
            at
        };
        let cursor = self.list.selected_index();
        let span: Vec<(i64, Picked)> = rows[at.min(cursor)..=at.max(cursor)]
            .iter()
            .map(|row| (row.key.thread_id, Picked::of(row)))
            .collect();
        let Some(extending) = self.bulk.extending.as_mut() else {
            return;
        };
        let selection = &mut self.actions.selection;
        let inside: BTreeSet<i64> = span.iter().map(|(id, _)| *id).collect();
        extending.added.retain(|id| {
            let keep = inside.contains(id);
            if !keep {
                selection.remove(*id);
            }
            keep
        });
        let fresh: Vec<(i64, Picked)> = (span.into_iter())
            .filter(|(id, _)| !selection.contains(*id))
            .collect();
        let full = selection.add(&fresh);
        let taken = fresh
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| selection.contains(*id));
        extending.added.extend(taken);
        // `(more match)` belongs to the capped select-all that set it.
        self.bulk.more = false;
        if full {
            self.say_full();
        }
    }
}

#[cfg(test)]
impl MailScreen {
    /// Range mode is on: `v` anchored it and neither `v` nor `Esc` has ended it.
    pub(crate) fn bulk_range_on(&self) -> bool {
        self.bulk.range_on()
    }
}
