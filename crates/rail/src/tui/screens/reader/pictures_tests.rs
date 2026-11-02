//! Pictures: `cid:` and remote addresses, sizing, decoding under limits, where a picture's
//! rows or placeholder stand among a rich body's lines, and remote ones held back until loaded.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use image::{DynamicImage, RgbaImage};

use rail_core::config::ThreadOrder;
use ratatui_image::picker::Picker;

use super::lines::{LineKind, ReaderLine};
use super::pictures::{
    DecodedPicture, MAX_KEPT_PIXELS, MAX_PICTURE_ROWS, MAX_PICTURES_IN_FLIGHT,
    MAX_PICTURES_PER_READER, Picture, Pictures, Source, content_id, decode, fit,
};
use super::rich::{Placing, from_html, lines};
use super::test_threads::{four, message, person, reader_of};

/// The hint the screen builds from the default keys.
const HINT: &str = "Remote pictures are not shown; i loads them, I always for this sender.";

/// Cells of 8 by 16 pixels, as many terminals have.
const FONT: (u16, u16) = (8, 16);

fn picture(width: u32, height: u32) -> Picture {
    Picture::Ready(Rc::new(DecodedPicture {
        image: DynamicImage::ImageRgba8(RgbaImage::new(width, height)),
        natural: (width, height),
    }))
}

fn built(html: &str, states: Option<&Pictures>) -> Vec<ReaderLine> {
    built_holding(html, states, None)
}

/// `built`, with remote pictures held back and `hint` saying how to load them.
fn built_holding(html: &str, states: Option<&Pictures>, hint: Option<&str>) -> Vec<ReaderLine> {
    let body = from_html(html);
    let placing = states.map(|states| Placing {
        states,
        font: FONT,
        message_id: 7,
        remote_hint: hint,
    });
    let mut out = Vec::new();
    lines(&mut out, 0, &body, &HashSet::new(), 80, placing.as_ref());
    out
}

fn cid(id: &str) -> (i64, Source) {
    (7, Source::Inline(id.into()))
}

fn texts(lines: &[ReaderLine]) -> Vec<&str> {
    lines.iter().map(ReaderLine::text).collect()
}

#[test]
fn a_cid_address_names_its_content_id_percent_decoded() {
    assert_eq!(content_id("cid:logo@x").as_deref(), Some("logo@x"));
    assert_eq!(content_id("CID:a%40b%2Ec").as_deref(), Some("a@b.c"));
    assert_eq!(content_id("https://x/logo.png"), None);
    assert_eq!(content_id("cid:"), None);
    assert_eq!(content_id("cid:a%20b"), None, "a space is no Content-ID");
}

#[test]
fn a_picture_takes_its_declared_size_shrunk_to_the_pane_and_the_row_cap() {
    // 160 by 64 CSS pixels: 20 columns, 4 rows of 16-pixel cells 8 wide.
    assert_eq!(
        fit((320, 128), (Some(160), Some(64)), 80, FONT),
        Some((20, 4))
    );
    // The picture's own size when the mail declares none, keeping its shape from one side.
    assert_eq!(fit((160, 64), (None, None), 80, FONT), Some((20, 4)));
    assert_eq!(fit((320, 128), (Some(160), None), 80, FONT), Some((20, 4)));
    // Wider than the pane: shrunk to it.
    assert_eq!(fit((1600, 640), (None, None), 50, FONT), Some((50, 10)));
    // Taller than the cap: shrunk to it, keeping its shape.
    let (_, rows) = fit((100, 4000), (None, None), 80, FONT).unwrap();
    assert_eq!(rows, MAX_PICTURE_ROWS);
    // A tracking pixel takes no room.
    assert_eq!(fit((1, 1), (None, None), 80, FONT), None);
}

#[test]
fn decoding_reads_a_real_picture_and_refuses_one_past_the_limits() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("small.png");
    RgbaImage::new(3, 2).save(&small).unwrap();
    let decoded = decode(&small).unwrap();
    assert_eq!((decoded.image.width(), decoded.image.height()), (3, 2));
    assert_eq!(decoded.natural, (3, 2));

    let huge = dir.path().join("huge.png");
    image::GrayImage::new(9000, 1).save(&huge).unwrap();
    assert!(decode(&huge).is_err(), "wider than Rail decodes");
    let junk = dir.path().join("junk.png");
    std::fs::write(&junk, b"<svg onload=alert(1)>").unwrap();
    assert!(decode(&junk).is_err());
}

#[test]
fn a_picture_shows_its_alt_text_until_read_then_its_rows_where_it_stood() {
    let html =
        r#"<p>Before</p><img src="cid:logo@x" alt="Logo" width="160" height="64"><p>After</p>"#;
    assert_eq!(
        texts(&built(html, Some(&HashMap::new()))),
        ["Before", "[image: Logo]", "", "After"]
    );

    let states = HashMap::from([(cid("logo@x"), picture(320, 128))]);
    let out = built(html, Some(&states));
    let kinds: Vec<LineKind> = out.iter().map(|l| l.kind).collect();
    let rows: Vec<LineKind> = (0..4)
        .map(|row| LineKind::Picture {
            message: 0,
            picture: 0,
            row,
            cols: 20,
            rows: 4,
        })
        .collect();
    assert_eq!(kinds[1..5], rows[..]);
    assert_eq!(texts(&out)[0], "Before");
    assert_eq!(texts(&out)[6], "After");

    let failed = HashMap::from([(cid("logo@x"), Picture::Failed)]);
    assert_eq!(texts(&built(html, Some(&failed)))[1], "[image: Logo]");
}

#[test]
fn a_picture_inside_a_line_breaks_it_and_images_off_leave_the_text_alone() {
    let html = r#"<p>one <img src="cid:icon@x" alt="icon"> two</p>"#;
    assert_eq!(
        texts(&built(html, Some(&HashMap::new()))),
        ["one", "[image: icon]", " two"]
    );
    assert_eq!(texts(&built(html, None)), ["one two"]);
}

#[test]
fn a_line_a_picture_breaks_still_draws_one_stored_line() {
    let html = r#"<p>one <img src="cid:icon@x" alt="icon"> two</p><p>three</p>"#;
    let sources: Vec<_> = built(html, Some(&HashMap::new()))
        .iter()
        .map(ReaderLine::source)
        .collect();
    // "one", the placeholder, " two", the paragraph break, "three".
    assert_eq!(
        sources,
        [Some((0, 1)), None, Some((0, 1)), Some((1, 2)), Some((2, 3))]
    );
}

#[test]
fn tracking_pixels_unnamed_remote_pictures_and_tiny_declared_ones_take_no_room() {
    let html = r#"<p>Hi</p><img src="cid:pixel@x" width="1" height="1">
                  <img src="https://tracker.example/p.gif"><p>Bye</p>"#;
    assert_eq!(
        texts(&built(html, Some(&HashMap::new()))),
        ["Hi", "", "Bye"]
    );
    let tiny = HashMap::from([(cid("dot@x"), picture(2, 2))]);
    let html = r#"<p>Hi</p><img src="cid:dot@x"><p>Bye</p>"#;
    assert_eq!(texts(&built(html, Some(&tiny))), ["Hi", "", "Bye"]);
}

#[test]
fn pictures_do_not_move_the_folds() {
    let html = r#"<img src="cid:logo@x"><blockquote>a<br>b<br>c<br>d</blockquote>"#;
    for states in [None, Some(&HashMap::new())] {
        let fold = built(html, states)
            .into_iter()
            .find(|l| matches!(l.kind, LineKind::Fold { .. }))
            .expect("the quote folds");
        assert_eq!(
            fold.kind,
            LineKind::Fold {
                message: 0,
                block: 0
            }
        );
    }
}

#[test]
fn a_large_picture_is_kept_shrunk_with_its_own_size_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let large = dir.path().join("large.png");
    image::GrayImage::new(2400, 1200).save(&large).unwrap();
    let decoded = decode(&large).unwrap();
    assert_eq!(decoded.natural, (2400, 1200), "sized as the mail's own");
    let (w, h) = (decoded.image.width(), decoded.image.height());
    assert!(u64::from(w) * u64::from(h) <= MAX_KEPT_PIXELS, "{w}x{h}");
    assert_eq!(w, h * 2, "its shape kept");
}

/// A reader in rich mode with images on whose newest message carries `n` pictures.
fn reader_with_pictures(n: usize) -> super::Reader {
    let html: String = (0..n)
        .map(|i| format!(r#"<p>{i}</p><img src="cid:p{i}@x" width="160" height="64">"#))
        .collect();
    let mut reader = reader_of(four(0), ThreadOrder::Oldest)
        .with_rich(true)
        .with_images(Some(Picker::halfblocks()));
    reader.rich_read(4, Some(from_html(&html)));
    reader
}

#[test]
fn a_reader_asks_for_a_few_pictures_at_a_time_and_no_more_than_its_cap() {
    let mut reader = reader_with_pictures(MAX_PICTURES_PER_READER + 8);
    let mut asked = 0;
    loop {
        let wanted = reader.pictures_wanted();
        assert!(wanted.len() <= MAX_PICTURES_IN_FLIGHT, "{}", wanted.len());
        if wanted.is_empty() {
            break;
        }
        reader.mark_pictures_asked(&wanted);
        assert!(
            reader.pictures_wanted().is_empty(),
            "none more while they load"
        );
        asked += wanted.len();
        for (id, _, source) in wanted {
            reader.picture_read(id, source, None);
        }
    }
    assert_eq!(asked, MAX_PICTURES_PER_READER);
}

#[test]
fn a_picture_keeps_one_encoding_whatever_sizes_it_is_drawn_at() {
    let mut reader = reader_with_pictures(1);
    let decoded = DecodedPicture {
        image: DynamicImage::ImageRgba8(RgbaImage::new(160, 64)),
        natural: (160, 64),
    };
    reader.picture_read(4, Source::Inline("p0@x".into()), Some(decoded));
    for (cols, rows) in [(20, 4), (10, 2), (40, 8), (20, 4)] {
        assert!(reader.protocol(3, 0, cols, rows).is_some());
        assert_eq!(reader.protocols.borrow().len(), 1);
    }
}

#[test]
fn pictures_of_a_message_gone_from_the_thread_do_not_hold_up_the_rest() {
    let mut reader = reader_with_pictures(MAX_PICTURES_IN_FLIGHT);
    let wanted = reader.pictures_wanted();
    assert_eq!(wanted.len(), MAX_PICTURES_IN_FLIGHT);
    reader.mark_pictures_asked(&wanted);
    // The newest message leaves the thread while its pictures load (their answers are
    // dropped), and a new one with pictures arrives.
    let mut detail = four(0);
    detail.messages.pop();
    detail.messages.push(message(
        5,
        person(None, "x@example.com"),
        1_760_080_000,
        "New.",
    ));
    reader.refresh(detail);
    let html = r#"<p>New</p><img src="cid:new@x" width="160" height="64">"#;
    reader.rich_read(5, Some(from_html(html)));
    assert_eq!(reader.pictures_wanted().len(), 1);
}

#[test]
fn a_picture_is_inline_by_its_content_id_or_remote_by_its_https_address() {
    let html = r#"<img src="cid:logo@x"><img src=" HTTPS://cdn.example.com/a.png ">
                  <img src="http://cdn.example.com/b.png"><img src="data:image/png;base64,AA">
                  <img src="//cdn.example.com/c.png">"#;
    let sources: Vec<Source> = from_html(html)
        .pictures
        .into_iter()
        .map(|p| p.source)
        .collect();
    assert_eq!(
        sources,
        [
            Source::Inline("logo@x".into()),
            Source::Remote("HTTPS://cdn.example.com/a.png".into()),
        ],
        "plain http, data and scheme-relative addresses are never fetched"
    );
}

#[test]
fn held_back_remote_pictures_leave_a_hint_and_their_alt_text_until_loaded() {
    let html = r#"<p>Hi</p><img src="https://cdn.example.com/sale.png" alt="Sale" width="160"
                  height="64"><img src="https://t.example.com/o.gif"><p>Bye</p>"#;
    let held = built_holding(html, Some(&HashMap::new()), Some(HINT));
    assert_eq!(texts(&held), [HINT, "Hi", "[image: Sale]", "", "Bye"]);
    assert!(matches!(held[0].kind, LineKind::Body { message: 0 }));
    // Loaded (or images off): no hint; loading, the alt text stands in.
    assert_eq!(
        texts(&built(html, Some(&HashMap::new()))),
        ["Hi", "[image: Sale]", "", "Bye"]
    );
    assert_eq!(
        texts(&built_holding(html, None, Some(HINT))),
        ["Hi", "", "Bye"],
        "images off: no hint"
    );
    // A body with only inline pictures has nothing to load.
    let inline = r#"<p>Hi</p><img src="cid:logo@x" alt="Logo">"#;
    assert_eq!(
        texts(&built_holding(inline, Some(&HashMap::new()), Some(HINT)))[0],
        "Hi"
    );
}

#[test]
fn a_remote_picture_read_takes_its_rows_as_an_inline_one_does() {
    let url = "https://cdn.example.com/sale.png";
    let html = format!(r#"<p>Hi</p><img src="{url}" alt="Sale" width="160" height="64">"#);
    let states = HashMap::from([((7, Source::Remote(url.into())), picture(320, 128))]);
    let rows = built(&html, Some(&states))
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Picture { .. }))
        .count();
    assert_eq!(rows, 4);
}

#[test]
fn a_reader_asks_for_remote_pictures_only_once_they_are_loaded() {
    let html = r#"<p>a</p><img src="cid:logo@x" width="160" height="64">
                  <img src="https://cdn.example.com/a.png" width="160" height="64">"#;
    let mut reader = reader_of(four(0), ThreadOrder::Oldest)
        .with_rich(true)
        .with_images(Some(Picker::halfblocks()))
        .with_remote(false, HashSet::new(), HINT.into());
    reader.rich_read(4, Some(from_html(html)));
    let sources = |reader: &super::Reader| -> Vec<Source> {
        reader.pictures_wanted().into_iter().map(|w| w.2).collect()
    };
    assert_eq!(sources(&reader), [Source::Inline("logo@x".into())]);

    assert!(reader.load_remote(), "something to load");
    assert_eq!(
        sources(&reader),
        [
            Source::Inline("logo@x".into()),
            Source::Remote("https://cdn.example.com/a.png".into())
        ]
    );
    assert!(!reader.load_remote(), "already loaded");

    let mut always = reader_of(four(0), ThreadOrder::Oldest)
        .with_rich(true)
        .with_images(Some(Picker::halfblocks()))
        .with_remote(true, HashSet::new(), HINT.into());
    always.rich_read(4, Some(from_html(html)));
    assert_eq!(
        always.pictures_wanted().len(),
        2,
        "remote_images loads them from the start"
    );
}

#[test]
fn a_trusted_senders_remote_pictures_load_without_the_key_and_others_wait() {
    // Message 4 is David's; Tom's message 2 is collapsed.
    let html = r#"<p>a</p><img src="https://cdn.example.com/a.png" width="160" height="64">"#;
    let trusted = HashSet::from(["david@example.com".to_string()]);
    let mut detail = four(0);
    detail.messages[3].vouched_domains = vec!["example.com".into()];
    let mut reader = reader_of(detail, ThreadOrder::Oldest)
        .with_rich(true)
        .with_images(Some(Picker::halfblocks()))
        .with_remote(false, trusted, HINT.into());
    reader.rich_read(4, Some(from_html(html)));
    assert_eq!(reader.pictures_wanted().len(), 1, "David is trusted");
    assert!(
        !reader.lines(100).iter().any(|l| l.text() == HINT),
        "no hint on a trusted sender's message"
    );

    reader.set_trusted(HashSet::from(["tom@example.com".to_string()]));
    assert!(reader.pictures_wanted().is_empty(), "David no longer is");
    assert!(reader.lines(100).iter().any(|l| l.text() == HINT));
}

/// A From line can be forged: a trusted sender's pictures load by themselves only when the
/// receiving server vouched for the sender's domain. Otherwise the message waits with the hint,
/// as an untrusted sender's does, and `i` still loads it.
#[test]
fn a_trusted_sender_whose_domain_was_not_vouched_for_waits_with_the_hint() {
    let html = r#"<p>a</p><img src="https://cdn.example.com/a.png" width="160" height="64">"#;
    let trusted = HashSet::from(["david@example.com".to_string()]);
    for vouched in [vec![], vec!["other.example".to_string()]] {
        let mut detail = four(0);
        detail.messages[3].vouched_domains = vouched;
        let mut reader = reader_of(detail, ThreadOrder::Oldest)
            .with_rich(true)
            .with_images(Some(Picker::halfblocks()))
            .with_remote(false, trusted.clone(), HINT.into());
        reader.rich_read(4, Some(from_html(html)));
        assert!(reader.pictures_wanted().is_empty());
        assert!(reader.lines(100).iter().any(|l| l.text() == HINT));
        assert!(reader.load_remote(), "i still loads them");
        assert_eq!(reader.pictures_wanted().len(), 1);
    }
}

#[test]
fn the_sender_at_the_cursor_is_its_messages_first_address_in_lower_case() {
    let mut detail = four(0);
    detail.messages[3].from = vec![person(Some("David"), "David@Example.COM")];
    let reader = reader_of(detail, ThreadOrder::Oldest);
    assert_eq!(
        reader.sender_at_cursor().as_deref(),
        Some("david@example.com")
    );
}
