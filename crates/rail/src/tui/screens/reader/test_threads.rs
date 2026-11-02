//! A four-message thread for the reader's builder and key tests, on the fixed clock: Priya
//! (Oct 03), Tom (Mon 22:11), David starred and unread (Wed 18:20), and David's newest (today
//! 04:32) to Vito and dana@example.com, in the AXON account.

use rail_core::config::{Icons, ThreadOrder};
use rail_core::provider::Mailbox;
use rail_core::store::view::{BodyState, MessageView, ThreadDetail};
use ratatui::style::Color;

use super::{Reader, ReaderAccount, unread_ids};
use crate::tui::clock::Clock;
use crate::tui::screens::mail::Glyphs;
use crate::tui::test_util::NOW;

pub const SUBJECT: &str = "Q4 roadmap review";

pub fn person(name: Option<&str>, address: &str) -> Mailbox {
    Mailbox {
        name: name.map(str::to_string),
        address: address.to_string(),
    }
}

/// A read message `id` from `from`, received and dated at `secs`; `text` is its body and its
/// snippet.
pub fn message(id: i64, from: Mailbox, secs: i64, text: &str) -> MessageView {
    MessageView {
        id,
        remote_id: format!("m{id}"),
        from: vec![from],
        to: Vec::new(),
        cc: Vec::new(),
        subject: SUBJECT.into(),
        date: secs,
        internal_date: secs * 1000,
        is_read: true,
        is_starred: false,
        snippet: text.into(),
        body: BodyState::Text {
            text: text.into(),
            cut: false,
        },
        links: Vec::new(),
        attachments: Vec::new(),
        has_html: false,
        vouched_domains: Vec::new(),
    }
}

/// The thread, `older_left_out` older messages not shown.
pub fn four(older_left_out: usize) -> ThreadDetail {
    let david = person(Some("David Hallas"), "david@example.com");
    let mut wednesday = message(3, david.clone(), 1_759_947_600, "Moving it to Thursday?");
    wednesday.is_read = false;
    wednesday.is_starred = true;
    let mut newest = message(4, david, 1_760_070_720, "Thursday works.");
    newest.is_read = false;
    newest.to = vec![
        person(Some("Vito"), "vito@example.com"),
        person(None, "dana@example.com"),
    ];
    let priya = person(Some("Priya Nair"), "priya@example.com");
    let tom = person(Some("Tom"), "tom@example.com");
    ThreadDetail {
        thread_id: 7,
        account_id: 1,
        subject: SUBJECT.into(),
        messages: vec![
            message(1, priya, 1_759_482_900, "Agenda attached."),
            message(2, tom, 1_759_788_660, "Sounds good."),
            wednesday,
            newest,
        ],
        older_left_out,
        snoozed_until: None,
    }
}

/// The thread's account: AXON, yellow.
pub fn axon() -> ReaderAccount {
    ReaderAccount {
        index: 0,
        color: Some(Color::Yellow),
        badge: "A".into(),
        label: "axon".into(),
    }
}

/// `NOW` in UTC, 24-hour times.
pub fn clock() -> Clock {
    Clock::fixed(NOW, jiff::tz::TimeZone::UTC)
}

/// A reader of `detail` in `order` on the fixed clock, Unicode glyphs, AXON. No frame is drawn,
/// so it works at 80 columns.
/// The fixture's unread messages are the ones unread at open, as the screen hands them over.
pub fn reader_of(detail: ThreadDetail, order: ThreadOrder) -> Reader {
    let unread = unread_ids(&detail);
    Reader::new(detail, clock(), Glyphs::of(Icons::Unicode), axon(), order).with_unread(unread)
}
