//! Whole-frame render tests.
//!
//! These drive the real widgets through `TestBackend` and assert on the buffer
//! that comes back, so they catch layout regressions that unit tests over the
//! pure functions cannot see — misaligned columns, a marker on the wrong line,
//! chrome that overruns its row.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::data::{Table, parse_csv};
use crate::layout::field_cap;
use crate::state::{Action, State, Viewport, update};
use crate::ui::{Context, Theme, render};

/// A fixture with the awkward cases the real sample file exercises.
fn table() -> Table {
    parse_csv(
        "id,customer,notes,total\n\
         1,Ada,\"first line\nsecond line\nthird line\nfourth line\",10\n\
         2,\u{5c71}\u{7530},short,20\n\
         3,Bob,,30\n\
         4,Kim,\"   \",40\n"
            .as_bytes(),
        "test.csv",
    )
    .expect("parse failed")
}

fn viewport(width: u16, height: u16) -> Viewport {
    let body = usize::from(height).saturating_sub(2);
    Viewport {
        width: usize::from(width).saturating_sub(crate::ui::record::GUTTER),
        full_width: usize::from(width),
        height: body,
        cap: field_cap(body, 40),
    }
}

/// Render one frame and return it as lines of text, trailing spaces trimmed.
fn frame(state: &State, table: &Table, width: u16, height: u16) -> Vec<String> {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("terminal construction failed");
    let view = viewport(width, height);
    let ctx = Context::new(Theme::new(false), table);
    terminal
        .draw(|f| render(f, state, table, view, &ctx))
        .expect("draw failed");

    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer.cell((x, y)).map_or(" ", |c| c.symbol()).to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn apply(actions: &[Action], table: &Table, width: u16, height: u16) -> State {
    let view = viewport(width, height);
    actions
        .iter()
        .fold(State::new(true), |s, a| update(&s, table, view, *a))
}

#[test]
fn record_view_renders_its_chrome_and_first_field() {
    let lines = frame(&State::new(true), &table(), 60, 12);

    assert!(lines[0].starts_with(" test.csv"), "file name in the bar");
    assert!(lines[0].contains("row 1/4"), "position in the bar");
    assert!(lines[0].ends_with("RECORD"), "mode in the bar");
    assert_eq!(
        lines[1],
        "\u{258c} id                                                     1/4"
    );
    assert_eq!(lines[2], "\u{258c} 1");
    assert!(lines[11].contains("h/l row"), "hints in the footer");
    assert!(
        lines[11].ends_with('\u{2026}'),
        "hints clip with an ellipsis"
    );
}

#[test]
fn the_selection_marker_follows_the_selected_field() {
    let table = table();
    let state = apply(&[Action::Down], &table, 60, 12);
    let lines = frame(&state, &table, 60, 12);

    assert!(!lines[1].starts_with('\u{258c}'), "id is no longer marked");
    assert!(lines[4].starts_with('\u{258c}'), "customer is marked");
    assert!(lines[4].contains("customer"));
}

#[test]
fn a_clamped_field_shows_the_remainder_marker() {
    let table = table();
    // A short screen forces the four-line notes field past its cap.
    let state = apply(&[Action::Down, Action::Down], &table, 60, 10);
    let lines = frame(&state, &table, 60, 10);

    let marker = lines
        .iter()
        .find(|l| l.contains("more lines"))
        .expect("expected a remainder marker");
    assert!(marker.contains('\u{22ef}'));
    assert!(marker.contains("open"), "says how to see the rest");
}

#[test]
fn placeholders_are_distinguishable() {
    let table = table();
    // Row 3 has an empty note, row 4 a whitespace-only one.
    let empty = apply(&[Action::Right, Action::Right], &table, 60, 14);
    assert!(
        frame(&empty, &table, 60, 14)
            .iter()
            .any(|l| l.contains("(empty)")),
        "empty field renders its placeholder"
    );

    let blank = apply(
        &[Action::Right, Action::Right, Action::Right],
        &table,
        60,
        14,
    );
    assert!(
        frame(&blank, &table, 60, 14)
            .iter()
            .any(|l| l.contains("(whitespace)")),
        "whitespace-only field is distinguishable from empty"
    );
}

#[test]
fn pager_fills_the_screen_with_one_field() {
    let table = table();
    let state = apply(&[Action::Down, Action::Down, Action::Enter], &table, 60, 10);
    let lines = frame(&state, &table, 60, 10);

    assert!(lines[0].contains("row 1"), "row in the bar");
    assert!(lines[0].contains("notes"), "column in the bar");
    assert!(lines[0].contains("lines 1-4/4"), "line counter in the bar");
    assert_eq!(lines[1], "first line");
    assert_eq!(lines[4], "fourth line");
    assert!(lines[9].contains("j/k line"), "pager hints in the footer");
}

#[test]
fn table_view_aligns_every_row() {
    let table = table();
    let state = apply(&[Action::ToggleTable], &table, 70, 10);
    let lines = frame(&state, &table, 70, 10);

    assert!(lines[0].ends_with("TABLE"));
    assert!(lines[1].trim_start().starts_with('#'), "row-number column");

    // The column header and every data row must start their second column at
    // the same offset, including the row with double-width characters.
    let customer_at = lines[1].find("customer").expect("customer column");
    for row in &lines[2..6] {
        let cell = row.char_indices().nth(customer_at).map(|(i, _)| i);
        assert!(cell.is_some(), "row is at least as wide as the header");
    }
    assert!(lines[4].contains("(empty)"), "empty cells are marked");
}

#[test]
fn table_marks_embedded_newlines() {
    let table = table();
    let state = apply(&[Action::ToggleTable], &table, 70, 10);
    let lines = frame(&state, &table, 70, 10);
    assert!(
        lines.iter().any(|l| l.contains('\u{23ce}')),
        "a multi-line cell is flattened with a return mark"
    );
}

#[test]
fn help_overlay_covers_the_screen() {
    let table = table();
    let state = apply(&[Action::ToggleHelp], &table, 60, 12);
    let lines = frame(&state, &table, 60, 12);

    assert!(lines[0].starts_with('\u{250c}'), "boxed overlay");
    assert!(lines[0].contains("miolo"));
    assert!(lines.iter().any(|l| l.contains("Global")));
    assert!(lines[11].starts_with('\u{2514}'));
}

#[test]
fn an_open_prompt_takes_over_the_footer() {
    let table = table();
    let state = apply(
        &[
            Action::BeginSearch,
            Action::PromptPush('n'),
            Action::PromptPush('o'),
        ],
        &table,
        60,
        12,
    );
    let lines = frame(&state, &table, 60, 12);
    assert!(lines[11].starts_with(" /no"), "prompt replaces the hints");
}

#[test]
fn chrome_never_overruns_a_narrow_screen() {
    let table = table();
    for width in [20u16, 40, 80] {
        let lines = frame(&State::new(true), &table, width, 8);
        for (index, line) in lines.iter().enumerate() {
            assert!(
                crate::layout::display_width(line) <= usize::from(width),
                "line {index} overflows at width {width}: {line:?}"
            );
        }
    }
}

#[test]
fn a_tiny_terminal_still_renders() {
    // Nothing here should panic even when there is barely room for chrome.
    for (width, height) in [(1u16, 1u16), (2, 2), (4, 3), (10, 4)] {
        let lines = frame(&State::new(true), &table(), width, height);
        assert_eq!(lines.len(), usize::from(height));
    }
}

#[test]
fn an_empty_table_renders_without_rows() {
    let empty = parse_csv(b"a,b\n", "empty.csv").expect("parse failed");
    let lines = frame(&State::new(true), &empty, 40, 6);
    assert!(lines[0].contains("no rows"));
}

// -- editor ---------------------------------------------------------------

/// The fixture with somewhere to write back to, so the editor opens without
/// reporting why it could not save.
fn writable() -> Table {
    let mut table = table();
    table.origin.path = Some(std::path::PathBuf::from("/tmp/test.csv"));
    table
}

#[test]
fn the_editor_shows_the_field_and_how_to_leave_it() {
    let table = writable();
    let state = apply(
        &[Action::Down, Action::Down, Action::BeginEdit],
        &table,
        60,
        10,
    );
    let lines = frame(&state, &table, 60, 10);

    assert!(lines[0].starts_with(" test.csv"), "file name in the bar");
    assert!(lines[0].ends_with("EDIT"), "mode in the bar");
    assert!(lines[9].contains("^s save"), "hints in the footer");
    assert!(lines[9].contains("Esc cancel"));

    // The point of editing in place: the record is still around the field.
    assert!(
        lines.iter().any(|l| l.contains("customer")),
        "the neighbouring fields are still on screen"
    );
    let notes = lines
        .iter()
        .position(|l| l.contains("notes"))
        .expect("the edited field's header");
    assert_eq!(lines[notes + 1], "\u{258c} first line");
    assert_eq!(lines[notes + 2], "\u{258c} second line");
}

#[test]
fn editing_from_the_pager_keeps_the_full_screen_surface() {
    let table = writable();
    let state = apply(
        &[Action::Down, Action::Down, Action::Enter, Action::BeginEdit],
        &table,
        60,
        10,
    );
    let lines = frame(&state, &table, 60, 10);

    assert!(lines[0].contains("notes"), "column being edited");
    assert!(lines[0].contains("line 1/4"), "position within the field");
    assert!(lines[0].ends_with("EDIT"));
    assert_eq!(
        lines[1], "first line",
        "no gutter: the field has the screen"
    );
    assert!(
        !lines.iter().any(|l| l.contains("customer")),
        "a field read full screen is edited full screen"
    );
}

#[test]
fn typing_shows_up_and_marks_the_field_modified() {
    let table = writable();
    let state = apply(
        &[
            Action::BeginEdit,
            Action::EditInsert('4'),
            Action::EditInsert('2'),
        ],
        &table,
        60,
        10,
    );
    let lines = frame(&state, &table, 60, 10);

    assert_eq!(
        lines[2], "\u{258c} 421",
        "typed at the caret, ahead of the old text"
    );
    assert!(lines[0].contains("modified"), "the bar says so");
}

#[test]
fn an_inline_edit_wraps_within_the_record_body() {
    let table = writable();
    // Ten cells of body once the gutter and the caret's column are taken, so
    // "first line" no longer fits on one row.
    let state = apply(
        &[Action::Down, Action::Down, Action::BeginEdit],
        &table,
        12,
        14,
    );
    let lines = frame(&state, &table, 12, 14);

    assert!(
        lines.iter().all(|l| crate::layout::display_width(l) <= 12),
        "nothing overruns the frame"
    );
    assert!(lines.iter().any(|l| l.trim_end() == "\u{258c} first"));
    assert!(lines.iter().any(|l| l.trim_end() == "\u{258c} line"));
}

#[test]
fn editing_lifts_the_cap_the_record_view_applies() {
    let table = writable();
    // The notes field is four lines against a three-line cap.
    let capped = apply(&[Action::Down, Action::Down], &table, 60, 11);
    assert!(
        frame(&capped, &table, 60, 11)
            .iter()
            .any(|l| l.contains("more lines")),
        "clamped while reading"
    );

    let editing = apply(
        &[Action::Down, Action::Down, Action::BeginEdit],
        &table,
        60,
        11,
    );
    let lines = frame(&editing, &table, 60, 11);
    assert!(
        !lines.iter().any(|l| l.contains("more lines")),
        "and whole while editing"
    );
    assert!(lines.iter().any(|l| l.contains("fourth line")));
}

#[test]
fn a_source_that_cannot_be_saved_says_so_where_it_will_be_read() {
    // The fixture has no path, standing in for piped input. The reason has to
    // reach the footer: opening is the last moment the user can walk away
    // without having typed anything.
    let table = table();
    let state = apply(&[Action::BeginEdit], &table, 60, 10);
    let lines = frame(&state, &table, 60, 10);

    assert!(lines[9].contains("cannot save"), "{}", lines[9]);
    assert!(
        !lines[9].contains("^s save"),
        "the reason displaces the hints"
    );
}

#[test]
fn the_discard_question_replaces_the_hints() {
    let table = writable();
    let state = apply(
        &[
            Action::BeginEdit,
            Action::EditInsert('x'),
            Action::EditCancel,
        ],
        &table,
        60,
        10,
    );
    let lines = frame(&state, &table, 60, 10);
    assert!(lines[9].contains("Discard changes"), "{}", lines[9]);
    assert!(lines[9].contains("y / n"), "says how to answer");
}

#[test]
fn the_full_screen_editor_wraps_within_the_screen() {
    let table = writable();
    let state = apply(
        &[Action::Down, Action::Down, Action::Enter, Action::BeginEdit],
        &table,
        9,
        10,
    );
    let lines = frame(&state, &table, 9, 10);

    // Eight cells of text, with the ninth column left free for the caret.
    // The break keeps the word whole and the trailing space with it.
    assert_eq!(lines[1], "first");
    assert_eq!(lines[2], "line");
    assert!(
        lines.iter().all(|l| l.chars().count() <= 9),
        "nothing overruns the frame"
    );
}

#[test]
fn an_unsaved_table_says_so_in_the_record_bar() {
    let mut table = writable();
    table.dirty = true;
    let lines = frame(&State::new(true), &table, 60, 12);
    assert!(lines[0].contains("unsaved"), "{}", lines[0]);
}
