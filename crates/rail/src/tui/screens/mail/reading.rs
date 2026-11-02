//! The mail screen's reader side: opening a thread, fetching its bodies, the reader's keys,
//! links and attachments.

use rail_core::config::link_choices;
use rail_core::ops::{BodyFetchResponse, ErrorCode, MAX_BODY_FETCH, OpError};
use rail_core::store::view::{BodyState, HtmlBody, ThreadDetail};

use super::{Focus, Glyphs, MailScreen, error_line};
use crate::tui::app::Effect;
use crate::tui::data::{FILTER_DEBOUNCE, ReadError, Timer, timer};
use crate::tui::keys::{Action, View};
use crate::tui::modals::{
    AttachmentList, AttachmentRow, Command, Confirm, ErrorList, ErrorRow, LinkPrompt, Modal,
    ModalOutcome, Target,
};
use crate::tui::screens::reader::{Reader, ReaderAccount, ReaderOutcome, unread_ids};
use crate::tui::text::{escape, fit, link_lines, safe_link};

/// How much of a refused address the status bar repeats.
const SHOWN_LINK_CHARS: usize = 120;

impl MailScreen {
    pub(super) fn read_thread(&mut self, thread_id: i64) -> Effect {
        let seq = self.next_seq();
        self.latest_thread = seq;
        self.ctx.store.thread(seq, thread_id)
    }

    /// A failed read outside the list (`what`: Rail's word for it): the status bar and
    /// `:errors` get it, and the list keeps its rows. The escaped line, for the area that
    /// asked; `None` when there is nothing to show there (a store that is not set up says so
    /// in the list, and only list reads are ever stale).
    pub(super) fn side_read_failed(
        &mut self,
        error: ReadError,
        what: &'static str,
    ) -> Option<String> {
        let ReadError::Failed(error) = error else {
            return None;
        };
        tracing::warn!(code = ?error.code, what, "a mail screen read failed");
        let text = format!("The {what} could not be read: {}", error_line(&error));
        self.status
            .error(text.clone(), self.ctx.clock.now(), self.tick);
        Some(text)
    }

    pub(super) fn open_selected(&mut self) -> Vec<Effect> {
        let Some(thread_id) = self.list.selected().map(|r| r.key.thread_id) else {
            return Vec::new();
        };
        vec![self.read_thread(thread_id)]
    }

    pub(super) fn thread_read(
        &mut self,
        thread_id: i64,
        result: Result<Option<ThreadDetail>, ReadError>,
    ) -> Vec<Effect> {
        match result {
            Ok(Some(mut detail)) => {
                let mut marked = Vec::new();
                match self.reader.as_mut().filter(|r| r.thread_id() == thread_id) {
                    // A re-read after a store change: the focus stays where it is.
                    Some(reader) => reader.refresh(detail),
                    None => {
                        let glyphs = Glyphs::of(self.ctx.icons);
                        let account = self.reader_account(detail.account_id);
                        // Kept before the mark-read, so the cards show what was unread.
                        let unread = unread_ids(&detail);
                        marked = self.mark_read_on_open(&mut detail);
                        let reader = Reader::new(
                            detail,
                            self.ctx.clock.clone(),
                            glyphs,
                            account,
                            self.ctx.thread_order,
                        );
                        let reader = reader
                            .with_unread(unread)
                            .with_rich(self.ctx.rich_html)
                            .with_images(self.picker())
                            .with_remote(
                                self.ctx.remote_images,
                                self.trusted_senders(),
                                self.remote_hint(),
                            );
                        self.reader = Some(reader);
                        // A read landing under an open composer leaves it the keys.
                        if self.compose.is_none() {
                            self.focus = Focus::Reader;
                        }
                    }
                }
                let mut effects = self.fetch_wanted();
                effects.extend(self.load_rich());
                effects.extend(marked);
                effects.extend(self.resume_open());
                effects
            }
            Ok(None) => {
                let effects = self.close_reader();
                self.status
                    .notify("That thread is no longer in the store.", self.tick);
                effects
            }
            Err(error) => {
                // The open thread's re-read: the reader says so until a read succeeds.
                if let Some(text) = self.side_read_failed(error, "thread")
                    && let Some(reader) =
                        self.reader.as_mut().filter(|r| r.thread_id() == thread_id)
                {
                    reader.read_failed(text);
                }
                Vec::new()
            }
        }
    }

    /// The thread's account as the reader's title block shows it: its place, colour, badge and
    /// label; `ReaderAccount::unknown` when it is no longer configured.
    fn reader_account(&self, account_id: i64) -> ReaderAccount {
        self.account_index_of(account_id)
            .and_then(|index| self.accounts.get(index).map(|entry| (index, entry)))
            .map_or_else(ReaderAccount::unknown, |(index, entry)| ReaderAccount {
                index,
                color: entry.color,
                badge: entry.badge(),
                label: entry.label.clone(),
            })
    }

    /// One `body.fetch` per `MAX_BODY_FETCH` of the open thread's uncached messages.
    pub(crate) fn fetch_wanted(&mut self) -> Vec<Effect> {
        let Some(reader) = self.reader.as_mut() else {
            return Vec::new();
        };
        let wanted = reader.wanted();
        if wanted.is_empty() {
            return Vec::new();
        }
        let ids: Vec<i64> = wanted.iter().map(|(id, _)| *id).collect();
        let account = self
            .account_ids
            .iter()
            .position(|id| *id == Some(reader.account_id()))
            .and_then(|i| self.accounts.get(i))
            .map(|a| a.name.clone());
        reader.mark_fetching(&ids);
        let Some(account) = account else {
            let gone = OpError::not_found("this account is no longer configured");
            reader.fetch_failed(&gone);
            return Vec::new();
        };
        let thread = reader.thread_id();
        wanted
            .chunks(MAX_BODY_FETCH)
            .map(|chunk| {
                let ids = chunk.iter().map(|(_, remote)| remote.clone()).collect();
                self.ctx.link.fetch_bodies(thread, account.clone(), ids)
            })
            .collect()
    }

    pub(super) fn bodies_answered(
        &mut self,
        thread_id: i64,
        result: Result<BodyFetchResponse, OpError>,
    ) -> Vec<Effect> {
        match &result {
            Ok(_) => self.status.answered(),
            Err(error) => self.status.failed(error),
        }
        let mut effects = self.dirty();
        let Some(reader) = self.reader.as_mut().filter(|r| r.thread_id() == thread_id) else {
            return effects;
        };
        match result {
            Ok(response) => {
                let failures = reader.fetched(&response);
                // A fetch that did not produce the body `o` waited for ends the wait.
                if let Some(id) = self.pending_open
                    && self
                        .reader
                        .as_ref()
                        .is_some_and(|r| r.error_of(id).is_some())
                {
                    self.pending_open = None;
                }
                for failure in failures {
                    self.login_hint(&failure);
                }
                effects.push(self.read_thread(thread_id));
            }
            Err(error) => {
                tracing::warn!(code = ?error.code, "a body fetch failed");
                reader.fetch_failed(&error);
                self.pending_open = None;
            }
        }
        effects
    }

    /// NeedsAuth: the account's login hint in the status bar, once per session.
    fn login_hint(&mut self, failure: &OpError) {
        if failure.code != ErrorCode::Auth {
            return;
        }
        let account = self
            .reader
            .as_ref()
            .and_then(|r| {
                self.account_ids
                    .iter()
                    .position(|id| *id == Some(r.account_id()))
            })
            .and_then(|i| self.accounts.get(i))
            .map(|a| a.name.clone())
            .unwrap_or_default();
        if self.status.first_login_hint(&account) {
            self.status
                .error(error_line(failure), self.ctx.clock.now(), self.tick);
        }
    }

    pub(super) fn reader_action(&mut self, action: Action) -> Vec<Effect> {
        let Some(reader) = self.reader.as_mut() else {
            self.focus = Focus::List;
            return Vec::new();
        };
        match reader.act(action) {
            // Opening a message reads its HTML for rich mode.
            ReaderOutcome::Stay => return self.load_rich(),
            ReaderOutcome::Back => return self.close_reader(),
            ReaderOutcome::FollowLink => self.follow_link(),
            ReaderOutcome::Attachments => self.attachments(),
            ReaderOutcome::SaveAttachments => return self.save_cursor_attachments(),
            ReaderOutcome::OpenBrowser => return self.open_html(),
            ReaderOutcome::TrustSender => return self.trust_sender(),
        }
        Vec::new()
    }

    /// Close the reader and any modal over it; a previewing picker puts the kept theme back,
    /// whose effect this returns.
    #[must_use]
    pub(super) fn close_reader(&mut self) -> Vec<Effect> {
        self.reader = None;
        self.modal = None;
        self.open_files.remove_all();
        self.opened_attachments.close(self.tick);
        self.pending_open = None;
        // An answer to a read still in flight must not reopen the thread.
        self.latest_thread = self.next_seq();
        // The composer stays over the list when the thread it answers goes.
        self.focus = if self.compose.is_some() {
            Focus::Compose
        } else {
            Focus::List
        };
        self.restore_theme()
    }

    /// `o` on the message at the cursor (or the latest).
    pub(super) fn open_html(&mut self) -> Vec<Effect> {
        let Some(reader) = self.reader.as_ref() else {
            return Vec::new();
        };
        let Some(message) = reader
            .message_at_cursor()
            .or_else(|| reader.detail().messages.last())
        else {
            return Vec::new();
        };
        let (id, body, has_html) = (message.id, message.body.clone(), message.has_html);
        match body {
            BodyState::NotFetched => self.open_after_fetch(id),
            _ if !has_html => {
                self.status.notify(
                    "This message has no HTML part; the reader shows all of its text.",
                    self.tick,
                );
                Vec::new()
            }
            _ => self.confirm_open(id),
        }
    }

    /// `o` on a message with no body yet: wait for a fetch that is running or start one; say
    /// so when none can happen (it failed before), instead of waiting for nothing.
    fn open_after_fetch(&mut self, id: i64) -> Vec<Effect> {
        let Some(reader) = self.reader.as_ref() else {
            return Vec::new();
        };
        if let Some(error) = reader.error_of(id) {
            let text = format!("Cannot open this message: {error}");
            self.status.notify(text, self.tick);
            return Vec::new();
        }
        let fetching = reader.is_fetching(id);
        if !fetching && !reader.wanted().iter().any(|(wanted, _)| *wanted == id) {
            self.status.notify(
                "Cannot open this message: its body is not available.",
                self.tick,
            );
            return Vec::new();
        }
        self.pending_open = Some(id);
        self.status
            .notify("fetching the message for the browser…", self.tick);
        self.fetch_wanted()
    }

    /// Ask once per session, naming remote images; then read the HTML.
    fn confirm_open(&mut self, message_id: i64) -> Vec<Effect> {
        if self.html_confirmed {
            return vec![self.ctx.store.html(message_id)];
        }
        let lines = vec![
            "Rail writes this message's HTML to a private temporary file and opens it in your browser."
                .to_string(),
            "Remote images in this message will load there, which can tell the sender you opened it."
                .to_string(),
            "Rail asks once per session; the file is removed when you close the reader."
                .to_string(),
        ];
        self.modal = Some(Modal::Confirm(Confirm::new(
            "Open in the browser?",
            lines,
            Command::OpenHtml(message_id),
        )));
        Vec::new()
    }

    /// After a re-read: the message `o` waited for has its body now (or failed).
    fn resume_open(&mut self) -> Vec<Effect> {
        let Some(id) = self.pending_open else {
            return Vec::new();
        };
        if let Some(error) = self.reader.as_ref().and_then(|r| r.error_of(id)) {
            let text = format!("Cannot open this message: {error}");
            self.pending_open = None;
            self.status.notify(text, self.tick);
            return Vec::new();
        }
        let found = self
            .reader
            .as_ref()
            .and_then(|r| r.detail().messages.iter().find(|m| m.id == id))
            .map(|m| (m.body.clone(), m.has_html));
        match found {
            Some((BodyState::NotFetched, _)) => Vec::new(),
            Some((_, true)) => {
                self.pending_open = None;
                self.confirm_open(id)
            }
            Some((_, false)) | None => {
                self.pending_open = None;
                self.status
                    .notify("This message has no HTML part to open.", self.tick);
                Vec::new()
            }
        }
    }

    pub(super) fn html_read(
        &mut self,
        message_id: i64,
        result: Result<Option<HtmlBody>, ReadError>,
    ) -> Vec<Effect> {
        // Only the open reader's own messages: the reader closed, or moved to another
        // thread, while the read was in flight.
        let ours = self
            .reader
            .as_ref()
            .is_some_and(|r| r.detail().messages.iter().any(|m| m.id == message_id));
        if !ours {
            return Vec::new();
        }
        match result {
            Ok(Some(HtmlBody::Html(html))) => self.open_file(&html),
            Ok(Some(HtmlBody::NotFetched)) => {
                self.pending_open = Some(message_id);
                return self.fetch_wanted();
            }
            Ok(Some(HtmlBody::Absent) | None) => {
                self.status
                    .notify("This message has no HTML part to open.", self.tick);
            }
            Err(error) => {
                self.side_read_failed(error, "message");
            }
        }
        Vec::new()
    }

    fn open_file(&mut self, html: &str) {
        let opened = self
            .open_files
            .write(html)
            .map_err(|e| {
                format!(
                    "Could not write the message for the browser: {}",
                    escape(&e.to_string())
                )
            })
            .and_then(|path| {
                url::Url::from_file_path(&path).map_err(|()| {
                    "Could not open the message: its path is not absolute".to_string()
                })
            })
            .and_then(|url| {
                (self.ctx.opener)(&url)
                    .map_err(|e| format!("Could not start the browser: {}", escape(&e.to_string())))
            });
        if let Err(text) = opened {
            tracing::warn!("could not open a message in the browser");
            self.status.error(text, self.ctx.clock.now(), self.tick);
        }
    }

    fn follow_link(&mut self) {
        let Some(reader) = self.reader.as_ref() else {
            return;
        };
        match reader.links_at_cursor() {
            None => self.status.notify("This message has no links.", self.tick),
            Some((links, numbers)) if numbers.len() == 1 => self.confirm_link(&links, numbers[0]),
            Some((links, _)) => self.modal = Some(Modal::LinkNumber(LinkPrompt::new(links))),
        }
    }

    /// The address the thread under the cursor names: `[[link_actions]]` run over its
    /// subject. One match is confirmed as any link is, several are offered by number.
    pub(super) fn open_subject_link(&mut self) -> Vec<Effect> {
        if self.ctx.link_actions.is_empty() {
            self.status
                .notify("No link actions are configured.", self.tick);
            return Vec::new();
        }
        let Some(subject) = self.subject_under_cursor() else {
            return Vec::new();
        };
        let found = link_choices(&self.ctx.link_actions, &subject);
        match found.len() {
            0 => self
                .status
                .notify("Nothing in this subject matches a link action.", self.tick),
            1 => self.confirm_url(&found[0].url),
            // Each match is offered, so a subject carrying two ids reaches either, and the
            // prompt names the rule behind each so the numbers mean something.
            _ => {
                let listed = found.into_iter().map(|one| (one.name, one.url)).collect();
                self.modal = Some(Modal::LinkNumber(LinkPrompt::listed(listed)));
            }
        }
        Vec::new()
    }

    /// The subject the cursor stands on: the open thread's, else the selected row's.
    fn subject_under_cursor(&self) -> Option<String> {
        if self.focus == Focus::Reader
            && let Some(reader) = self.reader.as_ref()
        {
            return Some(reader.detail().subject.clone());
        }
        self.list.selected().map(|row| row.subject.clone())
    }

    /// Show link `number`'s address before opening it; refuse one Rail would not open.
    fn confirm_link(&mut self, links: &[String], number: usize) {
        let Some(address) = links.get(number.wrapping_sub(1)) else {
            return;
        };
        self.confirm_url(address);
    }

    /// Show `address` before opening it; refuse one Rail would not open.
    fn confirm_url(&mut self, address: &str) {
        match safe_link(address) {
            Some(url) => {
                self.modal = Some(Modal::Confirm(Confirm::new(
                    "Open this link?",
                    link_lines(&url),
                    Command::OpenLink(url),
                )));
            }
            None => {
                let text = format!(
                    "Rail does not open this link: {}",
                    fit(address, SHOWN_LINK_CHARS)
                );
                tracing::warn!("refused to open a link Rail does not accept");
                self.status.error(text, self.ctx.clock.now(), self.tick);
            }
        }
    }

    fn attachments(&mut self) {
        let Some(reader) = self.reader.as_ref() else {
            return;
        };
        let detail = reader.detail();
        let account = self.account_name(detail.account_id).unwrap_or_default();
        let rows: Vec<AttachmentRow> = detail
            .messages
            .iter()
            .enumerate()
            .flat_map(|(index, m)| {
                let account = account.clone();
                m.attachments.iter().map(move |a| AttachmentRow {
                    account: account.clone(),
                    message: m.remote_id.clone(),
                    part_id: a.part_id.clone(),
                    name: a.filename.clone(),
                    mime_type: a.mime_type.clone(),
                    size: a.size,
                    message_index: index,
                })
            })
            .collect();
        if rows.is_empty() {
            self.status
                .notify("No attachments in this thread.", self.tick);
            return;
        }
        self.modal = Some(Modal::Attachments(AttachmentList::new(
            rows,
            self.ctx.icons,
        )));
    }

    pub(super) fn modal_outcome(&mut self, outcome: ModalOutcome) -> Vec<Effect> {
        match outcome {
            ModalOutcome::Close => self.close_modal(),
            ModalOutcome::Preview(Command::Theme(index)) => self.preview_theme(index),
            ModalOutcome::Preview(Command::Accent(accent)) => self.preview_accent(accent),
            // The modal keeps the key, and only the two pickers above preview anything.
            ModalOutcome::Stay | ModalOutcome::Preview(_) => Vec::new(),
            ModalOutcome::Typed(text) => {
                self.filter_text = text;
                self.filter_token += 1;
                vec![timer(FILTER_DEBOUNCE, self.filter_token, Timer::Filter)]
            }
            ModalOutcome::Run(command) => self.run_command(command),
        }
    }

    /// A modal answered with a command. Each arm closes the modal itself rather than one clear
    /// up here: the theme and accent arms close it where they write what was kept, and the
    /// attachment arms read which modal was open before it goes.
    fn run_command(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Theme(index) => return self.keep_scheme(index),
            Command::Accent(answer) => return self.keep_accent(answer),
            Command::Keymap(index) => return self.switch_keymap(index),
            Command::KeepKeymap => return self.keep_keymap(),
            Command::OpenAttachment(_)
            | Command::SaveAttachment(_)
            | Command::SaveAllAttachments(_) => return self.attachment_command(command),
            Command::ClearFilter => {
                self.modal = None;
                self.filter_text.clear();
                self.filter_token += 1;
                return self.apply_filter();
            }
            Command::Palette(target) => {
                self.modal = None;
                return self.run_palette(target);
            }
            Command::ChooseLink { links, number } => {
                self.modal = None;
                self.confirm_link(&links, number);
            }
            Command::OpenHtml(message_id) => {
                self.modal = None;
                self.html_confirmed = true;
                return vec![self.ctx.store.html(message_id)];
            }
            Command::OpenLink(url) => {
                self.modal = None;
                if let Err(error) = (self.ctx.opener)(&url) {
                    tracing::warn!(error = %error, "could not start the browser");
                    let text = format!(
                        "Could not start the browser: {}",
                        escape(&error.to_string())
                    );
                    self.status.error(text, self.ctx.clock.now(), self.tick);
                }
            }
            command => {
                self.modal = None;
                return self.action_command(command);
            }
        }
        Vec::new()
    }

    /// A modal closed without an answer: what it was waiting to apply goes, and a previewing
    /// picker puts the kept theme back.
    fn close_modal(&mut self) -> Vec<Effect> {
        self.modal = None;
        self.actions.picking = None;
        self.compose_modal_closed();
        let mut effects = self.restore_theme();
        // "Keep these keys?" answered no.
        effects.extend(self.revert_keymap());
        effects
    }

    /// Run a palette action as if its own view had focus: global and reader actions leave the
    /// reader open; others close it and move focus to the list or the sidebar.
    fn focus_for(&mut self, action: Action) -> Vec<Effect> {
        let views = action.views();
        if views.contains(&View::Global) || views.contains(&self.views()[0]) {
            return Vec::new();
        }
        let effects = if self.focus == Focus::Reader {
            self.close_reader()
        } else {
            Vec::new()
        };
        self.focus = if views.contains(&View::List) {
            Focus::List
        } else {
            Focus::Sidebar
        };
        effects
    }

    pub(super) fn run_palette(&mut self, target: Target) -> Vec<Effect> {
        match target {
            Target::Errors => {
                let rows = self
                    .status
                    .errors()
                    .rev()
                    .map(|e| ErrorRow {
                        text: format!("{}  {}", self.ctx.clock.short(e.at), e.text),
                        retry: e.retry.clone(),
                    })
                    .collect();
                self.modal = Some(Modal::Errors(ErrorList::with_retries(rows, self.ctx.icons)));
                Vec::new()
            }
            Target::Compose => self.start_compose(),
            Target::Drafts => self.drafts_picker(),
            Target::Theme => self.open_theme_picker(),
            Target::Accent => self.open_accent_picker(),
            Target::Keymap => self.open_keymap_picker(),
            Target::Hints => self.toggle_hints(),
            Target::Clock => self.toggle_clock(),
            Target::Time => self.toggle_time(),
            Target::ThreadOrder => self.toggle_thread_order(),
            Target::DimRead => self.cycle_dim_read(),
            Target::SaveSearch => self.save_search_prompt(),
            // Not the sidebar cursor's row: the palette has no cursor, so each entry names
            // its own search — and carries the name it showed, because the list can be
            // replaced while the palette is up.
            Target::RenameSearch { index, name } => self.rename_search_prompt(index, &name),
            Target::RemoveSearch { index, name } => self.remove_search_confirm(index, &name),
            Target::SearchCounts => self.toggle_search_counts(),
            Target::RichHtml => self.toggle_rich_html(),
            Target::Images => self.toggle_images(),
            Target::RemoteImages => self.toggle_remote_images(),
            Target::Scope(scope) => {
                let mut effects = self.close_reader();
                effects.extend(self.set_scope(scope));
                effects
            }
            Target::Action(Action::GlobalQuit) => vec![Effect::Quit],
            Target::Action(action) => {
                let mut effects = self.focus_for(action);
                effects.extend(self.act(action));
                effects
            }
        }
    }

    /// The message `o` waits on (tests).
    #[cfg(test)]
    pub(crate) fn pending_open(&self) -> Option<i64> {
        self.pending_open
    }

    /// Replace the browser opener (tests).
    #[cfg(test)]
    pub(crate) fn set_opener(&mut self, opener: crate::tui::effects::Opener) {
        self.ctx.opener = opener;
    }

    /// The open reader (tests).
    #[cfg(test)]
    pub(crate) fn reader(&self) -> Option<&Reader> {
        self.reader.as_ref()
    }

    /// The text of the reader's cursor line (tests).
    #[cfg(test)]
    pub(crate) fn reader_cursor_text(&self) -> Option<String> {
        self.reader
            .as_ref()?
            .cursor_line()
            .map(|l| l.text().to_string())
    }

    #[cfg(test)]
    pub(crate) fn error_count(&self) -> usize {
        self.status.errors().count()
    }
}
