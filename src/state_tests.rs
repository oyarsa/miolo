//! Tests for the state transition function.
//!
//! Split from `state.rs` to keep both files comfortably under the size limit.

use super::*;
use crate::data::parse;

/// The `notes` field is deliberately taller than the test viewport and wider
/// than half its width, so pager scrolling and horizontal shifting both have
/// somewhere to go. Lines stay narrower than the viewport so they do not wrap,
/// which keeps display indices equal to logical ones.
fn table() -> Table {
    let notes = (1..=12)
        .map(|n| format!("line {n:02} {}", "-".repeat(20)))
        .collect::<Vec<_>>()
        .join("\n");
    let csv = format!("id,notes,total\n1,\"{notes}\",10\n2,short,20\n3,other,30\n");
    parse(csv.as_bytes(), b',', "test").expect("parse failed")
}

fn view() -> Viewport {
    Viewport {
        width: 40,
        full_width: 42,
        height: 10,
        cap: 3,
    }
}

fn apply(state: &State, actions: &[Action]) -> State {
    let table = table();
    actions
        .iter()
        .fold(state.clone(), |s, a| update(&s, &table, view(), *a))
}

fn start() -> State {
    State::new(true)
}

fn typed(state: &State, text: &str) -> State {
    let actions: Vec<Action> = text.chars().map(Action::PromptPush).collect();
    apply(state, &actions)
}

#[test]
fn rows_move_and_stop_at_the_ends() {
    let s = apply(&start(), &[Action::Right]);
    assert_eq!(s.row, 1);
    let s = apply(&s, &[Action::Right, Action::Right]);
    assert_eq!(s.row, 2, "clamped at the last row");
    assert_eq!(s.status.as_deref(), Some("Last row"));

    let s = apply(&start(), &[Action::Left]);
    assert_eq!(s.row, 0);
    assert_eq!(s.status.as_deref(), Some("First row"));
}

#[test]
fn fields_move_and_stop_at_the_ends() {
    let s = apply(&start(), &[Action::Down, Action::Down]);
    assert_eq!(s.field, 2);
    let s = apply(&s, &[Action::Down]);
    assert_eq!(s.field, 2, "clamped at the last field");
    let s = apply(&s, &[Action::First]);
    assert_eq!(s.field, 0);
    let s = apply(&s, &[Action::Last]);
    assert_eq!(s.field, 2);
}

#[test]
fn expanding_toggles_and_targets_the_selection() {
    let s = apply(&start(), &[Action::Down, Action::ToggleExpand]);
    assert_eq!(s.expanded, Some(1));
    let s = apply(&s, &[Action::ToggleExpand]);
    assert_eq!(s.expanded, None);
}

#[test]
fn changing_row_collapses_an_expanded_field() {
    let s = apply(
        &start(),
        &[Action::Down, Action::ToggleExpand, Action::Right],
    );
    assert_eq!(s.expanded, None);
}

#[test]
fn wrap_toggles_and_reports() {
    let s = apply(&start(), &[Action::ToggleWrap]);
    assert!(!s.wrap);
    assert_eq!(s.status.as_deref(), Some("Truncate"));
    let s = apply(&s, &[Action::ToggleWrap]);
    assert!(s.wrap);
}

#[test]
fn turning_wrap_on_resets_the_pager_shift() {
    // Truncate mode first, then open the tall field and shift sideways.
    let s = apply(
        &start(),
        &[
            Action::ToggleWrap,
            Action::Down,
            Action::Enter,
            Action::Right,
        ],
    );
    assert!(!s.wrap);
    assert!(s.pager.shift > 0, "shifted while chopped");

    let s = apply(&s, &[Action::ToggleWrap]);
    assert!(s.wrap);
    assert_eq!(s.pager.shift, 0, "wrapping has nothing to shift against");
}

#[test]
fn scrolling_never_runs_past_the_content() {
    let s = apply(&start(), &[Action::ScrollDown(999)]);
    let body = body_for(&s, &table(), view());
    assert!(s.scroll <= body.len().saturating_sub(view().height));
}

#[test]
fn explicit_scrolling_does_not_drag_the_selection() {
    let s = apply(&start(), &[Action::HalfDown]);
    assert_eq!(s.field, 0, "scrolling leaves the selection alone");
}

#[test]
fn status_clears_on_the_next_action() {
    let s = apply(&start(), &[Action::Left]);
    assert!(s.status.is_some());
    let s = apply(&s, &[Action::Down]);
    assert!(s.status.is_none());
}

#[test]
fn quit_sets_the_flag() {
    assert!(apply(&start(), &[Action::Quit]).quit);
}

#[test]
fn empty_table_does_not_panic() {
    let empty = parse(b"a,b\n", b',', "test").expect("parse failed");
    let s = update(&start(), &empty, view(), Action::Right);
    assert_eq!(s.row, 0);
}

// -- pager ---------------------------------------------------------------

#[test]
fn enter_opens_the_pager_on_the_selected_field() {
    let s = apply(&start(), &[Action::Down, Action::Enter]);
    assert_eq!(s.mode, Mode::Pager);
    assert_eq!(s.pager.field, 1);
    assert_eq!(s.pager.scroll, 0);
}

#[test]
fn pager_scrolls_and_stops_at_the_ends() {
    let s = apply(&start(), &[Action::Down, Action::Enter, Action::Down]);
    assert_eq!(s.pager.scroll, 1);
    let s = apply(&s, &[Action::Up, Action::Up]);
    assert_eq!(s.pager.scroll, 0, "stops at the top");
}

#[test]
fn pager_back_returns_to_the_record() {
    let s = apply(&start(), &[Action::Enter, Action::Back]);
    assert_eq!(s.mode, Mode::Record);
}

#[test]
fn pager_shift_chops_and_returns_to_wrapping() {
    let s = apply(&start(), &[Action::Down, Action::Enter, Action::Right]);
    assert!(s.pager.shift > 0, "shifted right");
    let s = apply(&s, &[Action::Left]);
    assert_eq!(s.pager.shift, 0, "back to no shift");
}

#[test]
fn pager_shift_is_bounded_by_the_widest_line() {
    let s = apply(&start(), &[Action::Down, Action::Enter]);
    let s = apply(
        &s,
        &[Action::Right, Action::Right, Action::Right, Action::Right],
    );
    let widest = crate::layout::longest_line(s.pager_text(&table()));
    assert!(s.pager.shift <= widest, "never scrolls past the text");
}

// -- table ---------------------------------------------------------------

#[test]
fn table_toggles_both_ways_keeping_the_row() {
    let s = apply(&start(), &[Action::Right, Action::ToggleTable]);
    assert_eq!(s.mode, Mode::Table);
    assert_eq!(s.row, 1);
    let s = apply(&s, &[Action::ToggleTable]);
    assert_eq!(s.mode, Mode::Record);
    assert_eq!(s.row, 1, "selection is shared between the views");
}

#[test]
fn table_down_moves_by_row_not_field() {
    let s = apply(&start(), &[Action::ToggleTable, Action::Down]);
    assert_eq!(s.row, 1);
    assert_eq!(s.field, 0, "fields are not touched in the table");
}

#[test]
fn table_scrolls_columns() {
    let s = apply(&start(), &[Action::ToggleTable, Action::ColumnRight]);
    assert_eq!(s.column_offset, 1);
    let s = apply(&s, &[Action::ColumnLeft]);
    assert_eq!(s.column_offset, 0);
}

/// Every mode that is not the record view must act on `Back`, since that is
/// what both `q` and `Esc` decode to outside the record view. Testing the key
/// mapping alone missed that the table view ignored it.
#[test]
fn back_returns_to_the_record_from_every_mode() {
    for entry in [Action::ToggleTable, Action::Enter, Action::ToggleHelp] {
        let entered = apply(&start(), &[entry]);
        assert_ne!(entered.mode, Mode::Record, "{entry:?} left the record view");

        let returned = apply(&entered, &[Action::Back]);
        assert_eq!(
            returned.mode,
            Mode::Record,
            "Back did not return from {:?}",
            entered.mode
        );
    }
}

#[test]
fn table_enter_opens_the_record() {
    let s = apply(
        &start(),
        &[Action::ToggleTable, Action::Down, Action::Enter],
    );
    assert_eq!(s.mode, Mode::Record);
    assert_eq!(s.row, 1);
}

#[test]
fn table_keeps_the_selected_row_in_view() {
    let short = Viewport {
        height: 4,
        ..view()
    };
    let table = table();
    let mut s = update(&start(), &table, short, Action::ToggleTable);
    for _ in 0..3 {
        s = update(&s, &table, short, Action::Down);
    }
    assert!(s.row >= s.table_top);
    assert!(s.row < s.table_top + short.table_height().max(1));
}

// -- prompts and search --------------------------------------------------

#[test]
fn jump_moves_to_a_row() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "3");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.row, 2, "one-based on screen");
    assert!(s.prompt.is_none());
}

#[test]
fn jump_to_dollar_goes_to_the_last_row() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "$");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.row, 2);
}

#[test]
fn jump_out_of_range_is_rejected_without_moving() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "99");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.row, 0, "did not move");
    assert_eq!(s.status.as_deref(), Some("No such row: 99"));
}

#[test]
fn jump_rejects_nonsense() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "abc");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.row, 0);
    assert!(s.status.is_some());
}

#[test]
fn prompt_backspace_edits_then_clears() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "12");
    let s = apply(&s, &[Action::PromptPop]);
    assert_eq!(s.prompt, Some((Prompt::Jump, "1".to_owned())));
    let s = apply(&s, &[Action::PromptPop]);
    assert_eq!(s.prompt, None, "emptying the buffer closes the prompt");
}

#[test]
fn prompt_cancel_discards() {
    let s = apply(&start(), &[Action::BeginJump]);
    let s = typed(&s, "3");
    let s = apply(&s, &[Action::PromptCancel]);
    assert!(s.prompt.is_none());
    assert_eq!(s.row, 0);
}

#[test]
fn search_jumps_to_a_matching_column() {
    let s = apply(&start(), &[Action::BeginSearch]);
    let s = typed(&s, "tot");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.field, 2);
    assert_eq!(s.column_term, "tot");
}

#[test]
fn search_is_case_insensitive() {
    let s = apply(&start(), &[Action::BeginSearch]);
    let s = typed(&s, "NOTES");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.field, 1);
}

#[test]
fn search_reports_a_miss_without_moving() {
    let s = apply(&start(), &[Action::BeginSearch]);
    let s = typed(&s, "zzz");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.field, 0);
    assert_eq!(s.status.as_deref(), Some("No match: zzz"));
}

#[test]
fn search_term_persists_across_rows() {
    let s = apply(&start(), &[Action::BeginSearch]);
    let s = typed(&s, "tot");
    let s = apply(&s, &[Action::PromptSubmit, Action::Right]);
    assert_eq!(s.column_term, "tot", "term survives a row change");
    let s = apply(&s, &[Action::First, Action::NextMatch]);
    assert_eq!(s.field, 2, "n still finds it on the new row");
}

#[test]
fn next_match_without_a_term_reports_it() {
    let s = apply(&start(), &[Action::NextMatch]);
    assert_eq!(s.status.as_deref(), Some("No search term"));
}

#[test]
fn pager_search_moves_to_a_matching_line() {
    let s = apply(
        &start(),
        &[Action::Down, Action::Enter, Action::BeginSearch],
    );
    let s = typed(&s, "line 04");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert_eq!(s.pager.scroll, 3, "fourth line of the field");
    assert_eq!(s.content_term, "line 04");
}

#[test]
fn pager_search_does_not_touch_the_column_term() {
    let s = apply(
        &start(),
        &[Action::Down, Action::Enter, Action::BeginSearch],
    );
    let s = typed(&s, "line 04");
    let s = apply(&s, &[Action::PromptSubmit]);
    assert!(s.column_term.is_empty(), "column search is separate");
}

// -- help ----------------------------------------------------------------

#[test]
fn help_toggles_and_returns_to_the_previous_mode() {
    let s = apply(&start(), &[Action::ToggleTable, Action::ToggleHelp]);
    assert_eq!(s.mode, Mode::Help);
    let s = apply(&s, &[Action::ToggleHelp]);
    assert_eq!(s.mode, Mode::Table, "returns whence it came");
}

#[test]
fn help_closes_on_back() {
    let s = apply(&start(), &[Action::ToggleHelp, Action::Back]);
    assert_eq!(s.mode, Mode::Record);
}

// -- mouse ---------------------------------------------------------------

#[test]
fn clicking_selects_the_field_under_the_pointer() {
    let table = table();
    let s = start();
    let body = body_for(&s, &table, view());
    let target = body
        .iter()
        .position(|l| l.field == 1 && l.selectable())
        .expect("field 1 is in the body");
    let clicked = update(&s, &table, view(), Action::Click(target + RECORD_CHROME));
    assert_eq!(clicked.field, 1);
}

#[test]
fn clicking_the_status_bar_does_nothing() {
    let s = apply(&start(), &[Action::Click(0)]);
    assert_eq!(s.field, 0);
}

#[test]
fn clicking_a_table_row_selects_it() {
    let s = apply(&start(), &[Action::ToggleTable]);
    let s = apply(&s, &[Action::Click(TABLE_CHROME + 1)]);
    assert_eq!(s.row, 1);
}
