//! The palette's matching, help from the keymap, the filter line's keys, and their drawings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rail_core::config::Icons;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::style::Style;

use super::*;
use crate::tui::data::ScopeChoice;
use crate::tui::keys::{Action, Keymap, View};
use crate::tui::test_util::{key, render_lines, snapshot};
use crate::tui::theme::Theme;

fn entries() -> Vec<PaletteEntry> {
    let entry = |text: &str, id: &str, key: &str, target: Target| PaletteEntry {
        text: text.into(),
        search: format!("{text} {id}"),
        key: key.into(),
        target,
    };
    vec![
        entry(
            "Filter the list as you type",
            "list.filter",
            "/",
            Target::Action(Action::ListFilter),
        ),
        entry(
            "Show or hide the sidebar",
            "list.sidebar",
            "b",
            Target::Action(Action::ListSidebar),
        ),
        entry("Show recent errors", "errors", "", Target::Errors),
        entry(
            "Go to home's inbox",
            "account home",
            "2",
            Target::Scope(ScopeChoice::Account(1)),
        ),
        entry(
            "Label: work · Projects",
            "label",
            "",
            Target::Scope(ScopeChoice::Folder {
                account: 0,
                folder_id: 3,
                name: "Projects".into(),
                kind: rail_core::provider::FolderKind::User,
            }),
        ),
    ]
}

/// The default keys with `list.filter` moved to `x` (a key the list binds to nothing), so help
/// shows the override marker.
fn overridden_keymap() -> Keymap {
    let text = "[keys]\n\"list.filter\" = \"x\"\n";
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap()
}

fn typed(palette: &mut Palette, text: &str) {
    for c in text.chars() {
        assert_eq!(palette.key(key(KeyCode::Char(c))), ModalOutcome::Stay);
    }
}

#[test]
fn the_palette_matches_fuzzily_and_runs_the_best_entry() {
    let mut palette = Palette::new(entries());
    assert_eq!(palette.shown().len(), 5, "everything before typing");
    typed(&mut palette, "errors");
    assert_eq!(palette.shown()[0].target, Target::Errors);
    assert_eq!(
        palette.key(key(KeyCode::Enter)),
        ModalOutcome::Run(Command::Palette(Target::Errors))
    );
    let mut palette = Palette::new(entries());
    typed(&mut palette, "proj");
    assert!(matches!(
        palette.shown()[0].target,
        Target::Scope(ScopeChoice::Folder { .. })
    ));
    let mut palette = Palette::new(entries());
    typed(&mut palette, "zzzz");
    assert!(palette.shown().is_empty());
    assert_eq!(
        palette.key(key(KeyCode::Enter)),
        ModalOutcome::Stay,
        "nothing to run"
    );
    assert_eq!(palette.key(key(KeyCode::Esc)), ModalOutcome::Close);
}

#[test]
fn arrows_move_through_the_matches() {
    let mut palette = Palette::new(entries());
    palette.key(key(KeyCode::Down));
    palette.key(key(KeyCode::Down));
    assert_eq!(
        palette.key(key(KeyCode::Enter)),
        ModalOutcome::Run(Command::Palette(Target::Errors))
    );
}

#[test]
fn help_lists_the_view_and_the_global_keys_and_marks_overrides() {
    let help = Help::new(
        &overridden_keymap(),
        &[View::List, View::Global],
        Icons::Unicode,
    );
    let text = help.text();
    assert!(
        text.contains("Thread list") && text.contains("Everywhere"),
        "{text}"
    );
    assert!(
        text.contains("x *") && text.contains("Filter the list as you type"),
        "{text}"
    );
    assert_eq!(help.title(), "Keys (* set in config.toml)");
    assert!(
        !text.contains("reader.follow_link") && !text.contains("Open a link"),
        "{text}"
    );
    let reader = Help::new(
        &Keymap::defaults(),
        &[View::Reader, View::Global],
        Icons::Unicode,
    )
    .text();
    assert!(
        reader.contains("g x") && reader.contains("Open a link"),
        "{reader}"
    );
    assert!(!reader.contains('*'), "nothing overridden: {reader}");
    assert_eq!(
        Help::new(
            &Keymap::defaults(),
            &[View::Reader, View::Global],
            Icons::Unicode
        )
        .title(),
        "Keys"
    );
}

#[test]
fn the_filter_line_reports_each_change_and_esc_clears() {
    let mut line = FilterLine::new("");
    assert_eq!(
        line.key(key(KeyCode::Char('i'))),
        ModalOutcome::Typed("i".into())
    );
    for c in "s:unread form:dana".chars() {
        line.key(key(KeyCode::Char(c)));
    }
    assert_eq!(line.parsed(), "is:unread · words: form:dana");
    assert_eq!(line.key(key(KeyCode::Enter)), ModalOutcome::Close);
    assert_eq!(
        line.key(key(KeyCode::Esc)),
        ModalOutcome::Run(Command::ClearFilter)
    );
    assert_eq!(
        line.key(key(KeyCode::Backspace)),
        ModalOutcome::Typed("is:unread form:dan".into())
    );
}

#[test]
fn the_filter_line_stops_at_the_length_limit() {
    let mut line = FilterLine::new(&"x".repeat(rail_core::store::filter::MAX_FILTER_CHARS));
    assert_eq!(line.key(key(KeyCode::Char('y'))), ModalOutcome::Stay);
    assert_eq!(
        line.key(key(KeyCode::Backspace)),
        ModalOutcome::Typed("x".repeat(rail_core::store::filter::MAX_FILTER_CHARS - 1))
    );
}

#[test]
fn modal_snapshots() {
    let mut palette = Palette::new(entries());
    typed(&mut palette, "go");
    let modals = [
        ("palette", Modal::Palette(palette)),
        (
            "help",
            Modal::Help(Help::new(
                &overridden_keymap(),
                &[View::List, View::Global],
                Icons::Unicode,
            )),
        ),
        ("filter", {
            let mut line = FilterLine::new("");
            for c in "is:unread form:dana".chars() {
                line.key(key(KeyCode::Char(c)));
            }
            Modal::Filter(line)
        }),
        (
            "errors",
            Modal::Errors(ErrorList::new(
                vec![
                    "11:40  the connection to the daemon closed".into(),
                    "11:39  Rail does not open this link: javascript:alert(1)".into(),
                ],
                Icons::Unicode,
            )),
        ),
    ];
    for (name, modal) in &modals {
        for (width, height) in [(90, 24), (150, 40)] {
            let text = snapshot(width, height, |frame| {
                modal.render(frame, frame.area(), &Theme::default());
            });
            insta::assert_snapshot!(format!("modal_{name}_{width}x{height}"), text);
        }
    }
}

#[test]
fn a_confirmation_taller_than_the_screen_keeps_its_answer_line_and_cuts_with_an_ellipsis() {
    let lines = (0..30).map(|n| format!("line {n}")).collect();
    let confirm = Confirm::new("Open?", lines, Command::ClearFilter);
    for rows in [3u16, 4, 8, 12] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, rows)).unwrap();
        terminal
            .draw(|f| confirm.render(f, f.area(), &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row =
            |y: u16| -> String { (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>() };
        let last = rows - 2;
        assert!(row(last).contains("y yes"), "{rows}: {}", row(last));
        if rows >= 4 {
            assert!(
                row(last - 1).trim_end_matches(['│', ' ']).ends_with('…'),
                "{rows}: {}",
                row(last - 1)
            );
        }
    }
}

fn keymap_with(text: &str) -> Keymap {
    let path = std::path::Path::new("/c/config.toml");
    let config = rail_core::config::parse(text, path).unwrap();
    Keymap::from_config(&config.keys, config.general.keymap, text, path).unwrap()
}

#[test]
fn the_scroll_window_hints_only_at_hidden_rows() {
    use super::scroll::{Window, window};
    let w = |first, rows, above, below| Window {
        first,
        rows,
        above,
        below,
    };
    assert_eq!(window(5, 0, 10), w(0, 5, false, false), "everything fits");
    assert_eq!(window(30, 0, 10), w(0, 9, false, true));
    assert_eq!(window(30, 5, 10), w(6, 8, true, true));
    assert_eq!(
        window(30, 99, 10),
        w(21, 9, true, false),
        "clamped to the last page"
    );
    assert_eq!(
        window(30, 99, 2),
        w(28, 2, false, false),
        "no room for hints"
    );
    assert_eq!(window(0, 3, 0), w(0, 0, false, false));
}

#[test]
fn every_scroll_step_down_reveals_a_new_row() {
    use super::scroll::window;
    let (total, height) = (30, 10);
    let bottom = |offset| {
        let w = window(total, offset, height);
        w.first + w.rows - 1
    };
    let end = window(total, 1_000, height).first;
    let mut offset = 0;
    while window(total, offset, height).first < end {
        assert!(
            bottom(offset + 1) > bottom(offset),
            "offset {offset} to {} shows no new row",
            offset + 1
        );
        offset += 1;
    }
    assert_eq!(
        bottom(offset),
        total - 1,
        "the last step ends at the last row"
    );
    assert_eq!(window(total, 0, height).first, 0);
    assert_eq!(
        window(total, 1, height).first,
        2,
        "the hint takes row 1's place"
    );
}

#[test]
fn help_hints_at_hidden_rows_and_keeps_the_legend_in_its_title() {
    let mut help = Help::new(
        &overridden_keymap(),
        &[View::List, View::Global],
        Icons::Unicode,
    );
    let draw = |help: &Help| render_lines(90, 24, |f| help.render(f, f.area(), &Theme::default()));
    // At 90x24 the box spans rows 1-22, so its inside is rows 2-21.
    let rows = draw(&help);
    assert!(
        rows[1].contains("Keys (* set in config.toml)"),
        "{}",
        rows[1]
    );
    assert!(rows[21].contains("↓ more"), "{}", rows[21]);
    assert!(!rows.iter().any(|r| r.contains("↑ more")));
    for _ in 0..40 {
        help.key(key(KeyCode::Char('j')));
    }
    let rows = draw(&help);
    assert!(rows[2].contains("↑ more"), "{}", rows[2]);
    assert!(
        rows[21].contains("In the palette"),
        "the last page ends at the last row"
    );
    assert!(
        rows[1].contains("* set in config.toml"),
        "the legend scrolls with nothing"
    );
    help.key(key(KeyCode::Up));
    assert!(draw(&help)[21].contains("↓ more"));
    let ascii = Help::new(
        &overridden_keymap(),
        &[View::List, View::Global],
        Icons::Ascii,
    );
    assert!(draw(&ascii)[21].contains("v more"));
}

#[test]
fn help_closes_on_its_bound_key_esc_and_q() {
    let mut help = Help::new(
        &keymap_with("[keys]\n\"global.help\" = \"f3\"\n"),
        &[View::List, View::Global],
        Icons::Unicode,
    );
    assert_eq!(
        help.key(key(KeyCode::Char('?'))),
        ModalOutcome::Stay,
        "? is not the help key now"
    );
    assert_eq!(help.key(key(KeyCode::F(3))), ModalOutcome::Close);
    assert_eq!(help.key(key(KeyCode::Esc)), ModalOutcome::Close);
    assert_eq!(help.key(key(KeyCode::Char('q'))), ModalOutcome::Close);
    let mut default = Help::new(&Keymap::defaults(), &[View::Global], Icons::Unicode);
    let shifted = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT);
    assert_eq!(
        default.key(shifted),
        ModalOutcome::Close,
        "? as terminals send it"
    );
}

#[test]
fn errors_scroll_with_the_help_keys_and_hint_at_hidden_rows() {
    let rows = (0..40)
        .map(|n| format!("11:{n:02}  error number {n}"))
        .collect();
    let mut list = ErrorList::new(rows, Icons::Ascii);
    let draw =
        |list: &ErrorList| render_lines(90, 24, |f| list.render(f, f.area(), &Theme::default()));
    let shows = |rows: &[String], n: usize| {
        rows.iter()
            .any(|r| r.contains(&format!("error number {n} ")))
    };
    // At 90x24 the box spans rows 2-21, so its inside is rows 3-20.
    let rows = draw(&list);
    assert!(shows(&rows, 0) && shows(&rows, 16) && !shows(&rows, 17));
    assert!(rows[20].contains("v more"), "{}", rows[20]);
    assert_eq!(list.key(key(KeyCode::Char('j'))), ModalOutcome::Stay);
    let rows = draw(&list);
    assert!(rows[3].contains("^ more"), "{}", rows[3]);
    assert!(!shows(&rows, 0) && !shows(&rows, 1) && shows(&rows, 2) && shows(&rows, 17));
    list.key(key(KeyCode::Down));
    list.key(key(KeyCode::Char('k')));
    list.key(key(KeyCode::Up));
    assert!(shows(&draw(&list), 0), "back at the top");
    assert_eq!(list.key(key(KeyCode::Char('q'))), ModalOutcome::Close);
}

/// Where the terminal cursor is after drawing `modal` at 90x24 (`None`: hidden).
fn cursor_after(modal: &Modal) -> Option<Position> {
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    terminal
        .draw(|frame| modal.render(frame, frame.area(), &Theme::default()))
        .unwrap();
    let backend = terminal.backend();
    backend.cursor_visible().then(|| backend.cursor_position())
}

#[test]
fn the_palette_and_the_filter_line_draw_the_cursor_where_it_is() {
    let mut palette = Palette::new(entries());
    typed(&mut palette, "日本");
    let mut modal = Modal::Palette(palette);
    // At 90x24 the palette's inside starts at (11, 5); `:` takes column 11.
    assert_eq!(cursor_after(&modal), Some(Position::new(16, 5)));
    modal.key(key(KeyCode::Left));
    assert_eq!(
        cursor_after(&modal),
        Some(Position::new(14, 5)),
        "a wide character back"
    );
    modal.key(key(KeyCode::Home));
    assert_eq!(cursor_after(&modal), Some(Position::new(12, 5)));
    modal.key(key(KeyCode::Right));
    assert_eq!(cursor_after(&modal), Some(Position::new(14, 5)));
    // The filter line is the first row of its area; `/` takes column 0.
    let mut line = Modal::Filter(FilterLine::new("a\u{1b}b"));
    assert_eq!(
        cursor_after(&line),
        Some(Position::new(9, 0)),
        "after the escape's columns"
    );
    line.key(key(KeyCode::Left));
    line.key(key(KeyCode::Left));
    assert_eq!(cursor_after(&line), Some(Position::new(2, 0)));
    line.key(key(KeyCode::End));
    assert_eq!(cursor_after(&line), Some(Position::new(9, 0)));
}

fn token() -> rail_core::ops::RowToken {
    rail_core::ops::RowToken {
        row: 7,
        client_token: "tok".into(),
    }
}

#[test]
fn errors_with_a_retry_move_a_highlight_and_enter_retries_it() {
    let rows = vec![
        ErrorRow {
            text: "Could not archive: gone".into(),
            retry: Some(token()),
        },
        ErrorRow {
            text: "The store could not be read".into(),
            retry: None,
        },
    ];
    let mut list = ErrorList::with_retries(rows, Icons::Ascii);
    assert_eq!(
        list.key(key(KeyCode::Enter)),
        ModalOutcome::Run(Command::Retry(token()))
    );
    assert_eq!(list.key(key(KeyCode::Char('j'))), ModalOutcome::Stay);
    assert_eq!(
        list.key(key(KeyCode::Enter)),
        ModalOutcome::Close,
        "not retryable"
    );
}

/// "Rail default" and the first twenty schemes, Dracula ticked, with swatches or without
/// (as on a terminal without truecolor).
fn theme_picker(swatches: bool) -> Picker {
    use crate::tui::theme::scheme::schemes;
    let mut items = vec![PickerItem {
        id: -1,
        name: "Rail default".into(),
        checked: false,
        swatch: Vec::new(),
    }];
    items.extend(
        schemes()
            .iter()
            .take(20)
            .enumerate()
            .map(|(i, s)| PickerItem {
                id: i64::try_from(i).unwrap(),
                name: s.name.into(),
                checked: false,
                swatch: vec![
                    s.background.color(),
                    s.foreground.color(),
                    s.palette[6].color(),
                ],
            }),
    );
    let dracula = schemes().iter().position(|s| s.name == "Dracula").unwrap();
    items.push(PickerItem {
        id: i64::try_from(dracula).unwrap(),
        name: "Dracula".into(),
        checked: true,
        swatch: {
            let s = &schemes()[dracula];
            vec![
                s.background.color(),
                s.foreground.color(),
                s.palette[6].color(),
            ]
        },
    });
    if !swatches {
        items.iter_mut().for_each(|i| i.swatch.clear());
    }
    Picker::new(PickerKind::Theme, items)
}

#[test]
fn the_theme_and_accent_pickers_at_90_and_150_columns() {
    let theme = theme_picker(true);
    for (width, height) in [(90, 24), (150, 40)] {
        let text = snapshot(width, height, |frame| {
            theme.render(frame, frame.area(), &Theme::default());
        });
        insta::assert_snapshot!(format!("modal_theme_picker_{width}x{height}"), text);
    }
    let accents = (-1..16)
        .map(|n| PickerItem {
            id: n,
            name: if n < 0 {
                "scheme default".into()
            } else {
                format!("{n}")
            },
            checked: n == -1,
            swatch: u8::try_from(n)
                .map(|slot| vec![ratatui::style::Color::Indexed(slot)])
                .unwrap_or_default(),
        })
        .collect();
    let accent = Picker::new(PickerKind::Accent, accents);
    let text = snapshot(90, 24, |frame| {
        accent.render(frame, frame.area(), &Theme::default());
    });
    insta::assert_snapshot!("modal_accent_picker_90x24", text);
}

#[test]
fn a_scheme_row_draws_its_swatch_in_the_scheme_colours() {
    let picker = theme_picker(true);
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    terminal
        .draw(|frame| picker.render(frame, frame.area(), &Theme::default()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = (0..buffer.area.height)
        .find(|&y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains("Dracula")
        })
        .expect("the Dracula row is drawn");
    let first = (0..buffer.area.width)
        .find(|&x| buffer[(x, row)].symbol() == "█")
        .expect("a swatch cell");
    assert_eq!(
        buffer[(first, row)].fg,
        ratatui::style::Color::Rgb(0x28, 0x2a, 0x36)
    );
}

#[test]
fn a_modal_paints_the_themes_background_over_cells_no_span_covers() {
    use crate::tui::theme::choice::{Accent, ColorDepth, ThemeChoice};
    use crate::tui::theme::scheme::scheme;
    let theme = Theme::from_choice(
        ThemeChoice {
            scheme: scheme("Dracula"),
            accent: Accent::SchemeDefault,
        },
        ColorDepth::TrueColor,
    );
    let other = ratatui::style::Color::Rgb(1, 2, 3);
    let picker = theme_picker(true);
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            frame.buffer_mut().set_style(area, Style::new().bg(other));
            picker.render(frame, area, &theme);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let input_row = (0..buffer.area.height)
        .find(|&y| buffer[(0, y)].symbol() == " " && row_text(buffer, y).contains("> "))
        .expect("the input row is drawn");
    let x = (0..buffer.area.width)
        .find(|&x| buffer[(x, input_row)].symbol() == ">")
        .expect("the prompt");
    // The row under the input is a blank line: no span styles its cells.
    let blank = &buffer[(x + 3, input_row + 1)];
    assert_eq!(blank.symbol(), " ");
    assert_eq!(blank.bg, ratatui::style::Color::Rgb(0x28, 0x2a, 0x36));
}

fn row_text(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

#[test]
fn the_theme_picker_without_truecolor_at_90_columns() {
    let picker = theme_picker(false).without_preview();
    let text = snapshot(90, 24, |frame| {
        picker.render(frame, frame.area(), &Theme::default());
    });
    assert!(text.contains("no preview without truecolor"), "{text}");
    insta::assert_snapshot!("modal_theme_picker_no_preview_90x24", text);
}

/// The reader's links are numbered in the message text, so that prompt stays the bare
/// question it has always been: two lines in a box 48 columns wide.
#[test]
fn the_bare_link_prompt_is_the_question_alone() {
    let prompt = LinkPrompt::new(vec![
        "https://e.example/a".into(),
        "https://e.example/b".into(),
    ]);
    let lines = render_lines(80, 10, |frame| {
        prompt.render(frame, frame.area(), &Theme::default());
    });
    let box_rows: Vec<&String> = lines.iter().filter(|l| l.contains('│')).collect();
    assert_eq!(box_rows.len(), 2, "{lines:#?}");
    assert!(
        box_rows[0].contains("Open link number (1-2):"),
        "{box_rows:#?}"
    );
    assert!(
        box_rows[1].contains("Enter open · Esc cancel"),
        "{box_rows:#?}"
    );
    let top = lines.iter().find(|l| l.contains('┌')).expect("a border");
    let left = top.chars().position(|c| c == '┌').expect("a left corner");
    let right = top.chars().position(|c| c == '┐').expect("a right corner");
    assert_eq!(right - left + 1, 48, "{top}");
}

/// More matches than the screen can hold: the question and the hint are the rows that
/// matter, so they stay, and the listing is cut where it runs out of room.
#[test]
fn a_listing_taller_than_the_screen_keeps_the_question() {
    let matches: Vec<(String, String)> = (0..40)
        .map(|n| {
            (
                "tracker".to_string(),
                format!("https://tracker.example/t/{n}"),
            )
        })
        .collect();
    let prompt = LinkPrompt::listed(matches);
    let lines = render_lines(80, 8, |frame| {
        prompt.render(frame, frame.area(), &Theme::default());
    });
    let rendered = lines.join("\n");
    assert!(rendered.contains("Open link number (1-40):"), "{rendered}");
    assert!(rendered.contains("Enter open · Esc cancel"), "{rendered}");
    assert!(
        rendered.contains('…'),
        "the cut listing says so: {rendered}"
    );
}

/// The README's own rules: two addresses that differ only in the id at their end.
fn two_merge_requests() -> LinkPrompt {
    let address =
        |id: u32| format!("https://gitlab.example.com/group/project/-/merge_requests/{id}");
    LinkPrompt::listed(vec![
        ("Merge-request".to_string(), address(123)),
        ("Merge-request".to_string(), address(456)),
    ])
}

/// A wide terminal has the room, so an address is shown whole.
#[test]
fn a_listed_address_is_whole_when_the_terminal_has_room() {
    let prompt = two_merge_requests();
    let rendered = render_lines(150, 12, |frame| {
        prompt.render(frame, frame.area(), &Theme::default());
    })
    .join("\n");
    assert!(rendered.contains("/merge_requests/123"), "{rendered}");
    assert!(rendered.contains("/merge_requests/456"), "{rendered}");
}

/// A narrow terminal cuts the middle: the id at the end is what tells two addresses apart.
#[test]
fn a_cut_listed_address_keeps_its_end() {
    let prompt = two_merge_requests();
    let rendered = render_lines(60, 12, |frame| {
        prompt.render(frame, frame.area(), &Theme::default());
    })
    .join("\n");
    assert!(rendered.contains('…'), "{rendered}");
    assert!(rendered.contains("requests/123"), "{rendered}");
    assert!(rendered.contains("requests/456"), "{rendered}");
    assert!(rendered.contains("1  Merge-request"), "{rendered}");
}
