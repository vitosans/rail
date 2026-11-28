//! The shell owns classic Kitty images from the reader's draw through terminal restore.

use std::cell::Cell;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use futures_util::stream;
use ratatui::backend::TestBackend;
use ratatui::layout::{Rect, Size};
use ratatui::{Frame, Terminal};

use super::app::{App, Effect, Msg};
use super::classic_kitty::{Bitmap, Placement};
use super::handover::{Handover, Suspend};
use super::run::{redraw, run_loop};
use super::screen::Screen;
use super::terminal::{Restored, TerminalGuard};
use super::test_util::{RecordingClipboard, once};
use super::theme::Theme;

#[derive(Default, Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn picture() -> Placement {
    let image = image::DynamicImage::new_rgba8(4, 6);
    let bitmap = Bitmap::encode(&image, Size::new(2, 3), (2, 2)).unwrap();
    Placement::clipped(bitmap, Rect::new(1, 2, 6, 5), 0).unwrap()
}

struct PictureScreen {
    drawn: Cell<bool>,
    picture: Placement,
}

impl Screen for PictureScreen {
    fn update(&mut self, _: Msg) -> Vec<Effect> {
        vec![]
    }

    fn view(&self, _: &mut Frame, _: &Theme) {
        self.drawn.set(true);
    }

    fn classic_placements(&self) -> Vec<Placement> {
        assert!(self.drawn.replace(false), "read placements before a frame");
        vec![self.picture.clone()]
    }
}

#[tokio::test]
async fn classic_images_are_removed_on_input_error_and_end_of_stream() {
    for error in [false, true] {
        let screen = PictureScreen {
            drawn: Cell::new(false),
            picture: picture(),
        };
        let mut app = App::new(screen, Box::new(RecordingClipboard::default()));
        let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();
        let output = Capture::default();
        let mut hand = Handover::for_terminal(Restored::default(), output.clone());
        let events = error.then(|| Err(io::Error::other("input ended")));
        let result = run_loop(
            &mut terminal,
            &mut app,
            once(stream::iter(events)),
            &mut hand,
        )
        .await;
        assert_eq!(result.is_err(), error);
        let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        assert!(written.contains("a=T") || written.contains("a=t"));
        assert_eq!(written.matches("d=I").count(), 1, "{written:?}");
    }
}

#[test]
fn an_editor_handover_hides_classic_images_before_leaving() {
    let output = Capture::default();
    let mut hand = Handover::for_terminal(Restored::default(), output.clone())
        .with_steps(|| Err(io::Error::other("editor unavailable")), || Ok(()));
    hand.draw_classic(&[picture()]).unwrap();
    assert!(hand.suspend().is_err());
    let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    assert_eq!(written.matches("d=I").count(), 1, "{written:?}");
}

#[test]
fn dropping_the_terminal_clears_owned_images_before_leaving_the_screen() {
    let output = Capture::default();
    let state = Restored::default();
    let guard = TerminalGuard::new(output.clone(), state.clone());
    let mut hand = Handover::for_terminal(state, output.clone());
    hand.draw_classic(&[picture()]).unwrap();
    drop(guard);
    let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    assert!(written.find("d=I").unwrap() < written.find("\x1b[?1049l").unwrap());
}

/// #213: ratatui clears the screen when the terminal size changes, which takes the
/// placements with it, so the next redraw must put every visible picture back.
#[test]
fn a_resize_places_classic_images_again() {
    let screen = PictureScreen {
        drawn: Cell::new(false),
        picture: picture(),
    };
    let app = App::new(screen, Box::new(RecordingClipboard::default()));
    let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();
    let output = Capture::default();
    let mut hand = Handover::for_terminal(Restored::default(), output.clone());
    let written =
        |output: &Capture| String::from_utf8(output.0.lock().unwrap().split_off(0)).unwrap();

    redraw(&mut terminal, &app, &mut hand).unwrap();
    assert_eq!(written(&output).matches("a=p,").count(), 1);
    redraw(&mut terminal, &app, &mut hand).unwrap();
    assert!(
        written(&output).is_empty(),
        "an unchanged frame resends nothing"
    );

    terminal.backend_mut().resize(20, 14);
    redraw(&mut terminal, &app, &mut hand).unwrap();
    let after = written(&output);
    assert_eq!(after.matches("a=t,").count(), 1, "{after:?}");
    assert_eq!(after.matches("a=p,").count(), 1, "{after:?}");
}
