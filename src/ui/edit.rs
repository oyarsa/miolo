//! The field editor's full-screen surface, and the chrome both surfaces share.
//!
//! The inline surface is the record view with the buffer spliced into it, so
//! it lives in `record.rs`; only the footer and the cursor placement are
//! common enough to keep here.
//!
//! Like every other view this one decides nothing. The caret's screen position
//! is computed by `state`, from the same wrap the body is drawn with, so the
//! block the terminal shows and the offset the buffer holds cannot drift apart.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::data::Table;
use crate::edit::{self, Editing, Surface};
use crate::layout::truncate_to_width;
use crate::state::{State, Viewport};
use crate::ui::{Theme, justify, split, thousands};

pub const HINTS: &str =
    "^s save  Esc cancel  \u{21b5} newline  \u{2190}\u{2191}\u{2193}\u{2192} move";
const CONFIRM: &str = "Discard changes to this field?  y / n";

/// Draw the editor.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    table: &Table,
    view: Viewport,
    theme: Theme,
) {
    let [status_area, body_area, footer_area] = split(area);
    let width = area.width as usize;
    let Some(editing) = &state.editing else {
        return;
    };

    frame.render_widget(status_bar(state, editing, table, theme, width), status_area);
    frame.render_widget(Paragraph::new(body(editing, view)), body_area);
    frame.render_widget(footer(state, editing, theme, width), footer_area);

    let (row, column) = edit::caret_position(&editing.buffer, view.edit_width(Surface::FullScreen));
    place_cursor(
        frame,
        body_area,
        editing,
        row.saturating_sub(editing.scroll),
        column,
    );
}

/// Put the terminal's own cursor on the caret.
///
/// A real cursor rather than a drawn block, so it blinks and sits where the
/// user's terminal puts every other cursor. A pending question owns the
/// keyboard, so the cursor goes away until it is answered.
pub fn place_cursor(frame: &mut Frame, body: Rect, editing: &Editing, row: usize, column: usize) {
    if editing.confirming || row >= body.height as usize {
        return;
    }
    frame.set_cursor_position(Position::new(
        body.x + u16::try_from(column).unwrap_or(u16::MAX),
        body.y + u16::try_from(row).unwrap_or(u16::MAX),
    ));
}

/// `file · row 42 · notes  •modified    line 3/60   EDIT`
pub fn status_bar(
    state: &State,
    editing: &Editing,
    table: &Table,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let mut left = vec![Span::raw(format!(
        " {} \u{b7} row {} \u{b7} {}",
        table.name,
        thousands(state.row + 1),
        table.column_name(editing.field)
    ))];
    if editing.modified() {
        left.push(Span::styled("  \u{2022}modified", theme.marker()));
    }

    // Counted in logical lines, not wrapped rows: the number stays put when
    // the terminal is resized, which is what makes it worth reporting.
    let text = &editing.buffer.text;
    let total = text.split('\n').count();
    let line = text[..editing.buffer.caret].matches('\n').count();
    let right = vec![
        Span::raw(format!("line {}/{}", thousands(line + 1), thousands(total))),
        Span::raw("   EDIT "),
    ];

    justify(left, right, width).style(theme.bar())
}

/// The visible rows of the buffer.
pub fn body(editing: &Editing, view: Viewport) -> Vec<Line<'static>> {
    let text = &editing.buffer.text;
    edit::rows(text, view.edit_width(Surface::FullScreen))
        .into_iter()
        .skip(editing.scroll)
        .take(view.height)
        .map(|row| Line::from(Span::raw(edit::expand_tabs(&text[row.start..row.end]))))
        .collect()
}

/// The discard question wins, then a message, then the hints.
///
/// The message matters most when the editor has just opened on something that
/// cannot be saved: that is the one moment the user can still walk away
/// without having typed anything.
pub fn footer(state: &State, editing: &Editing, theme: Theme, width: usize) -> Line<'static> {
    if editing.confirming {
        return Line::from(vec![Span::styled(format!(" {CONFIRM}"), theme.warning())])
            .style(theme.bar());
    }
    let text = state.status.as_deref().unwrap_or(HINTS);
    let text = truncate_to_width(text, width.saturating_sub(1));
    justify(vec![Span::raw(format!(" {text}"))], Vec::new(), width).style(theme.bar())
}
