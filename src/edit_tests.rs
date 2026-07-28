//! Tests for the editor buffer.
//!
//! Split from `edit.rs` to keep both files comfortably under the size limit.

use super::*;

fn buffer(text: &str, caret: usize) -> Buffer {
    Buffer {
        text: text.to_owned(),
        caret,
    }
}

/// The rows put back together, newlines and all, must be the original text.
/// Everything else here depends on that, because a caret offset can only map
/// to one screen position if no byte was dropped or duplicated.
fn reassemble(text: &str, width: usize) -> String {
    let rows = rows(text, width);
    let mut out = String::new();
    let mut previous_end = None;
    for row in rows {
        if let Some(end) = previous_end {
            // Consecutive rows either continue a wrapped line or are separated
            // by exactly the newline that split them.
            assert!(row.start == end || row.start == end + 1);
            if row.start == end + 1 {
                out.push('\n');
            }
        }
        out.push_str(&text[row.start..row.end]);
        previous_end = Some(row.end);
    }
    out
}

#[test]
fn wrapping_preserves_every_byte() {
    let samples = [
        "",
        "hello",
        "hello world this is a longer line",
        "one\ntwo\nthree",
        "a\n\nb",
        "trailing newline\n",
        "\n\n\n",
        "日本語のテキストです ascii mixed in",
        "tabs\there\tand\tthere",
        "🎌 emoji 🎌 with 🎌 spaces",
        "supercalifragilisticexpialidocious",
    ];
    for text in samples {
        for width in [1, 2, 3, 5, 8, 20, 200] {
            assert_eq!(
                reassemble(text, width),
                *text,
                "text {text:?} at width {width}"
            );
        }
    }
}

#[test]
fn zero_width_behaves_like_one() {
    assert_eq!(rows("abc", 0), rows("abc", 1));
}

#[test]
fn an_empty_field_still_has_a_row() {
    assert_eq!(rows("", 10), [Row { start: 0, end: 0 }]);
}

#[test]
fn blank_lines_get_their_own_rows() {
    assert_eq!(rows("a\n\nb", 10).len(), 3);
}

#[test]
fn a_trailing_newline_opens_an_empty_last_row() {
    let rows = rows("a\n", 10);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1], Row { start: 2, end: 2 });
}

#[test]
fn wrapping_breaks_after_a_space_and_keeps_it() {
    // "aaa " is four cells and fits; "bbb" starts the next row.
    let text = "aaa bbb";
    let rows = rows(text, 5);
    assert_eq!(rows.len(), 2);
    assert_eq!(&text[rows[0].start..rows[0].end], "aaa ");
    assert_eq!(&text[rows[1].start..rows[1].end], "bbb");
}

#[test]
fn wrapping_breaks_mid_word_when_there_is_nowhere_else() {
    let text = "aaaaaaaa";
    let rows = rows(text, 3);
    assert_eq!(rows.len(), 3);
    assert_eq!(&text[rows[0].start..rows[0].end], "aaa");
    assert_eq!(&text[rows[2].start..rows[2].end], "aa");
}

#[test]
fn no_row_is_wider_than_the_width() {
    let text = "日本語です and some ascii words to break on";
    for width in [1, 2, 3, 4, 7, 11] {
        for row in rows(text, width) {
            let rendered = cells(&text[row.start..row.end]);
            // A single character wider than the width gets a row of its own,
            // which is the one case that may overflow.
            let single = text[row.start..row.end].chars().count() == 1;
            assert!(
                rendered <= width || single,
                "row {rendered} cells at width {width}"
            );
        }
    }
}

#[test]
fn a_wide_character_narrower_than_itself_still_advances() {
    // Two-cell glyphs in a one-cell terminal: one per row, no infinite loop.
    let rows = rows("日本語", 1);
    assert_eq!(rows.len(), 3);
}

#[test]
fn tabs_count_as_a_fixed_indent() {
    assert_eq!(cells("\t"), TAB_WIDTH);
    assert_eq!(cells("a\tb"), TAB_WIDTH + 2);
    assert_eq!(expand_tabs("a\tb"), "a    b");
}

#[test]
fn cells_measures_wide_characters() {
    assert_eq!(cells("日本"), 4);
    assert_eq!(cells("ab"), 2);
}

#[test]
fn the_caret_maps_to_a_screen_position() {
    let buffer = buffer("abc\ndef", 5);
    assert_eq!(caret_position(&buffer, 20), (1, 1));
}

#[test]
fn the_caret_at_a_line_end_stays_on_that_line() {
    let buffer = buffer("abc\ndef", 3);
    assert_eq!(caret_position(&buffer, 20), (0, 3));
}

#[test]
fn the_caret_after_a_wrap_point_lands_on_the_new_row() {
    // Caret at offset 4 is exactly where "aaa " broke, so it belongs to the
    // row that offset starts rather than the one it ended.
    let buffer = buffer("aaa bbb", 4);
    assert_eq!(caret_position(&buffer, 5), (1, 0));
}

#[test]
fn the_caret_column_accounts_for_wide_characters() {
    let buffer = buffer("日本x", 6);
    assert_eq!(caret_position(&buffer, 20), (0, 4));
}

#[test]
fn inserting_moves_the_caret_past_what_was_typed() {
    let out = insert(&buffer("ac", 1), 'b');
    assert_eq!(out, buffer("abc", 2));
}

#[test]
fn inserting_a_multibyte_character_advances_by_its_length() {
    let out = insert(&buffer("", 0), '日');
    assert_eq!(out.caret, '日'.len_utf8());
}

#[test]
fn inserting_a_newline_splits_the_line() {
    let out = insert(&buffer("ab", 1), '\n');
    assert_eq!(out.text, "a\nb");
    assert_eq!(caret_position(&out, 20), (1, 0));
}

#[test]
fn backspace_removes_a_whole_character() {
    assert_eq!(backspace(&buffer("日本", 6)), buffer("日", 3));
    assert_eq!(backspace(&buffer("ab", 2)), buffer("a", 1));
}

#[test]
fn backspace_at_the_start_does_nothing() {
    assert_eq!(backspace(&buffer("ab", 0)), buffer("ab", 0));
}

#[test]
fn delete_removes_the_character_under_the_caret() {
    assert_eq!(delete(&buffer("abc", 1)), buffer("ac", 1));
}

#[test]
fn delete_at_the_end_does_nothing() {
    assert_eq!(delete(&buffer("ab", 2)), buffer("ab", 2));
}

#[test]
fn deleting_joins_two_lines() {
    assert_eq!(delete(&buffer("a\nb", 1)), buffer("ab", 1));
}

#[test]
fn horizontal_movement_steps_by_character() {
    assert_eq!(right(&buffer("日本", 0)).caret, 3);
    assert_eq!(left(&buffer("日本", 3)).caret, 0);
}

#[test]
fn horizontal_movement_stops_at_the_ends() {
    assert_eq!(left(&buffer("ab", 0)).caret, 0);
    assert_eq!(right(&buffer("ab", 2)).caret, 2);
}

#[test]
fn vertical_movement_keeps_the_column() {
    let out = move_rows(&buffer("abcd\nefgh", 2), 20, 1);
    assert_eq!(caret_position(&out, 20), (1, 2));
}

#[test]
fn vertical_movement_clamps_to_a_shorter_line() {
    let out = move_rows(&buffer("abcd\nef", 4), 20, 1);
    assert_eq!(caret_position(&out, 20), (1, 2), "lands at the line end");
}

#[test]
fn vertical_movement_stops_at_the_first_and_last_row() {
    assert_eq!(move_rows(&buffer("ab\ncd", 1), 20, -1).caret, 1);
    let last = move_rows(&buffer("ab\ncd", 1), 20, 9);
    assert_eq!(caret_position(&last, 20).0, 1);
}

#[test]
fn vertical_movement_crosses_wrapped_rows() {
    // One logical line wrapped into three rows: moving down twice from the
    // top lands on the third row, not past the end of the field.
    let out = move_rows(&buffer("aaa bbb ccc", 0), 4, 2);
    assert_eq!(caret_position(&out, 4).0, 2);
}

#[test]
fn row_ends_bracket_the_visual_row() {
    let text = "aaa bbb";
    let start = row_start(&buffer(text, 5), 5);
    let end = row_end(&buffer(text, 5), 5);
    assert_eq!(start.caret, 4, "start of the wrapped row");
    assert_eq!(end.caret, 7, "end of the wrapped row");
}

#[test]
fn opening_a_field_normalises_line_endings_only() {
    let editing = Editing::new(0, 0, "a\r\nb\tc", Surface::Inline);
    assert_eq!(
        editing.buffer.text, "a\nb\tc",
        "tabs survive, CRLF does not"
    );
    assert!(!editing.modified(), "normalising is not an edit");
}

#[test]
fn opening_a_field_puts_the_caret_at_the_top() {
    let editing = Editing::new(0, 0, "abc\ndef", Surface::Inline);
    assert_eq!(caret_position(&editing.buffer, 20), (0, 0));
}

#[test]
fn typing_marks_the_field_modified() {
    let mut editing = Editing::new(3, 1, "abc", Surface::Inline);
    editing.buffer = insert(&editing.buffer, 'd');
    assert!(editing.modified());
    assert_eq!(editing.row, 3, "the target never moves");
    assert_eq!(editing.field, 1);
}
