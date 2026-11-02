//! Pictures through the mail screen: an open HTML message's `cid:` picture asked of the daemon
//! once, its rows kept and drawn, and `:images` hiding it, saved to `config.toml`; a remote
//! picture held back until `i` loads the thread's, `I` trusts its sender, or `:remote-images`
//! loads them always.

use crossterm::event::KeyCode;
use image::RgbaImage;
use rail_core::ops::AttachmentImageResponse;
use rail_core::store::Store;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use super::{Fixture, fixture, keys, open_toms_card, press, store_bodies_on_fetch};
use crate::tui::data::FakeCall;
use crate::tui::screen::Screen;
use crate::tui::screens::reader::LineKind;
use crate::tui::screens::reader::pictures::MAX_PICTURES_PER_READER;
use crate::tui::theme::Theme;

/// m1 shows a 160 by 64 picture under its first paragraph, whose bytes the fake daemon has.
async fn with_a_picture() -> (Fixture, tempfile::TempDir) {
    let f = fixture(false).await;
    store_bodies_on_fetch(&f);
    let html = r#"<p>Hi Dana,</p><img src="cid:logo@x" alt="Logo" width="160" height="64">
                  <p>Thanks,<br>Tom</p>"#;
    Store::open(&f.paths.db_file())
        .unwrap()
        .conn()
        .execute(
            "UPDATE messages SET body_html = ?1 WHERE remote_id = 'm1'",
            [html],
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("logo.png");
    RgbaImage::from_pixel(32, 16, image::Rgba([200, 30, 30, 255]))
        .save(&png)
        .unwrap();
    f.fake.image_answers.lock().unwrap().insert(
        "logo@x".into(),
        Ok(AttachmentImageResponse {
            path: png.display().to_string(),
            mime_type: "image/png".into(),
        }),
    );
    (f, dir)
}

fn image_calls(f: &Fixture) -> Vec<FakeCall> {
    f.fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::AttachmentImage { .. }))
        .cloned()
        .collect()
}

fn picture_rows(f: &Fixture) -> usize {
    f.screen
        .reader()
        .unwrap()
        .lines(80)
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Picture { .. }))
        .count()
}

async fn run_images(f: &mut Fixture) {
    press(f, KeyCode::Char(':')).await;
    keys(f, "images").await;
    press(f, KeyCode::Enter).await;
}

#[tokio::test]
async fn an_open_messages_picture_is_asked_for_once_and_drawn_in_its_rows() {
    let (mut f, _dir) = with_a_picture().await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(
        image_calls(&f),
        [FakeCall::AttachmentImage {
            account: "work".into(),
            message_id: "m1".into(),
            content_id: "logo@x".into(),
        }]
    );
    // 160 by 64 CSS pixels in the half-block picker's 10 by 20 cells: 20 columns, 4 rows.
    assert_eq!(picture_rows(&f), 4);
    // Half blocks draw a solid picture as cells in its colour.
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).unwrap();
    terminal
        .draw(|frame| f.screen.view(frame, &Theme::default()))
        .unwrap();
    let red = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.bg == Color::Rgb(200, 30, 30))
        .count();
    assert!(red >= 20 * 4 / 2, "{red} cells in the picture's colour");

    // Closing and opening the card again asks for nothing new.
    press(&mut f, KeyCode::Tab).await;
    press(&mut f, KeyCode::Tab).await;
    assert_eq!(image_calls(&f).len(), 1);
}

#[tokio::test]
async fn colon_images_hides_and_shows_the_pictures_and_saves_the_choice() {
    let (mut f, _dir) = with_a_picture().await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(picture_rows(&f), 4);

    run_images(&mut f).await;
    assert!(f.screen.status_line().contains("images: off"));
    assert_eq!(picture_rows(&f), 0);
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(text.contains("images = false"), "{text}");

    run_images(&mut f).await;
    assert_eq!(picture_rows(&f), 4, "drawn again from what was read");
    assert_eq!(image_calls(&f).len(), 1);
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(!text.contains("images"), "{text}");
}

#[tokio::test]
async fn a_mail_with_many_pictures_has_only_the_readers_cap_asked_for() {
    let (mut f, _dir) = with_a_picture().await;
    let many = MAX_PICTURES_PER_READER + 8;
    let html: String = (0..many)
        .map(|i| format!(r#"<p>{i}</p><img src="cid:p{i}@x" width="160" height="64">"#))
        .collect();
    Store::open(&f.paths.db_file())
        .unwrap()
        .conn()
        .execute(
            "UPDATE messages SET body_html = ?1 WHERE remote_id = 'm1'",
            [html],
        )
        .unwrap();
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    // Each answer lets the next one be asked for, until the cap.
    assert_eq!(image_calls(&f).len(), MAX_PICTURES_PER_READER);
}

const SALE: &str = "https://cdn.example.com/sale.png";

/// m1 shows a remote 160 by 64 picture, whose bytes the fake daemon has, Tom's domain vouched
/// for by the receiving server.
async fn with_a_remote_picture() -> (Fixture, tempfile::TempDir) {
    with_a_remote_picture_vouched(r#"["example.com"]"#).await
}

/// As `with_a_remote_picture`, the vouched domains column set to `vouched` (JSON, or NULL).
async fn with_a_remote_picture_vouched(vouched: &str) -> (Fixture, tempfile::TempDir) {
    let (f, dir) = with_a_picture().await;
    let html = format!(
        r#"<p>Hi Dana,</p><img src="{SALE}" alt="Sale" width="160" height="64"><p>Tom</p>"#
    );
    Store::open(&f.paths.db_file())
        .unwrap()
        .conn()
        .execute(
            "UPDATE messages SET body_html = ?1, vouched_domains = ?2 WHERE remote_id = 'm1'",
            [Some(html), (vouched != "NULL").then(|| vouched.to_string())],
        )
        .unwrap();
    let answer = f.fake.image_answers.lock().unwrap()["logo@x"].clone();
    f.fake
        .image_answers
        .lock()
        .unwrap()
        .insert(SALE.into(), answer);
    (f, dir)
}

fn remote_calls(f: &Fixture) -> Vec<FakeCall> {
    f.fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, FakeCall::RemoteImage(_)))
        .cloned()
        .collect()
}

fn has_hint(f: &Fixture) -> bool {
    f.screen.reader().unwrap().lines(80).iter().any(|l| {
        l.text() == "Remote pictures are not shown; i loads them, I always for this sender."
    })
}

#[tokio::test]
async fn a_remote_picture_waits_for_i_and_is_then_fetched_once_and_drawn() {
    let (mut f, _dir) = with_a_remote_picture().await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(remote_calls(&f), [], "nothing fetched before the key");
    assert!(has_hint(&f));
    assert_eq!(picture_rows(&f), 0);

    press(&mut f, KeyCode::Char('i')).await;
    assert_eq!(remote_calls(&f), [FakeCall::RemoteImage(SALE.into())]);
    assert!(!has_hint(&f));
    assert_eq!(picture_rows(&f), 4);

    press(&mut f, KeyCode::Char('i')).await;
    assert_eq!(remote_calls(&f).len(), 1, "loaded once");
}

#[tokio::test]
async fn colon_remote_images_loads_them_always_and_saves_the_choice() {
    let (mut f, _dir) = with_a_remote_picture().await;
    press(&mut f, KeyCode::Char(':')).await;
    keys(&mut f, "remote-images").await;
    press(&mut f, KeyCode::Enter).await;
    assert!(f.screen.status_line().contains("remote images: on"));
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(text.contains("remote_images = true"), "{text}");

    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(remote_calls(&f).len(), 1, "fetched without the key");
    assert!(!has_hint(&f));

    press(&mut f, KeyCode::Char(':')).await;
    keys(&mut f, "remote-images").await;
    press(&mut f, KeyCode::Enter).await;
    assert!(f.screen.status_line().contains("remote images: off"));
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(!text.contains("remote_images"), "{text}");
}

#[tokio::test]
async fn capital_i_trusts_the_sender_for_good_and_again_stops_trusting_them() {
    let (mut f, _dir) = with_a_remote_picture().await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(remote_calls(&f), []);

    press(&mut f, KeyCode::Char('I')).await;
    assert!(
        f.screen
            .status_line()
            .contains("remote pictures from tom@example.com: always"),
        "{}",
        f.screen.status_line()
    );
    assert_eq!(remote_calls(&f).len(), 1, "loaded at once");
    assert!(!has_hint(&f));
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(
        text.contains(r#"remote_images_from = ["tom@example.com"]"#),
        "{text}"
    );

    // Opened again, Tom's pictures load without a key.
    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(remote_calls(&f).len(), 2);

    press(&mut f, KeyCode::Char('I')).await;
    assert!(
        f.screen
            .status_line()
            .contains("tom@example.com: ask first")
    );
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(!text.contains("remote_images_from"), "{text}");
}

#[tokio::test]
async fn capital_i_keeps_a_sender_added_to_the_file_while_rail_was_open() {
    let (mut f, _dir) = with_a_remote_picture().await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    let config = f.paths.config_file();
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let by_hand = |list: &str| {
        let text = format!("[general]\nremote_images_from = [{list}] # by hand\n");
        std::fs::write(&config, text).unwrap();
    };
    by_hand(r#""ann@example.com""#);

    press(&mut f, KeyCode::Char('I')).await;
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(
        text.contains(r#"remote_images_from = ["ann@example.com", "tom@example.com"] # by hand"#),
        "{text}"
    );

    by_hand(r#""ann@example.com", "Tom@Example.com", "bob@example.com""#);
    press(&mut f, KeyCode::Char('I')).await;
    let text = std::fs::read_to_string(f.paths.config_file()).unwrap();
    assert!(
        text.contains(r#"remote_images_from = ["ann@example.com", "bob@example.com"] # by hand"#),
        "{text}"
    );
}

/// Mail whose From line the receiving server did not vouch for could be forged: `I` trusts the
/// sender and loads this message's pictures, as asked, but the next time it waits with the hint.
#[tokio::test]
async fn capital_i_on_unvouched_mail_loads_it_now_but_not_by_itself_again() {
    let (mut f, _dir) = with_a_remote_picture_vouched("NULL").await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    press(&mut f, KeyCode::Char('I')).await;
    assert!(
        f.screen
            .status_line()
            .contains("remote pictures from tom@example.com: always")
    );
    assert_eq!(remote_calls(&f).len(), 1, "loaded as asked");

    press(&mut f, KeyCode::Char('q')).await;
    press(&mut f, KeyCode::Enter).await;
    open_toms_card(&mut f).await;
    assert_eq!(remote_calls(&f).len(), 1, "not again without the key");
    assert!(has_hint(&f));
}
