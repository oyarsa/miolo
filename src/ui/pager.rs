//! The field pager: one field, full screen, with content search.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::data::Table;
use crate::markdown::{Segment, classify, has_fence, looks_structured};
use crate::state::{State, Viewport, pager_view};
use crate::ui::{Theme, footer, justify, split, thousands};

const HINTS: &str =
    "j/k line  \u{2190}/\u{2192} shift  ^d/^u page  g/G top/end  / search  e edit  q back";

/// Draw the pager.
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

    frame.render_widget(status_bar(state, table, view, theme, width), status_area);
    frame.render_widget(Paragraph::new(body(state, table, view, theme)), body_area);
    frame.render_widget(footer(state, table, theme, width, HINTS), footer_area);
}

/// `file · row 42 · notes    lines 24-48/312`
pub fn status_bar(
    state: &State,
    table: &Table,
    view: Viewport,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let column = table
        .headers
        .get(state.pager.field)
        .cloned()
        .unwrap_or_default();
    let left = vec![Span::raw(format!(
        " {} \u{b7} row {} \u{b7} {}",
        table.name,
        thousands(state.row + 1),
        column
    ))];

    let lines = pager_view(state, table, view);
    let first = state.pager.scroll + 1;
    let last = (state.pager.scroll + view.height).min(lines.len());
    let mut right = vec![Span::raw(format!(
        "lines {}-{}/{}",
        thousands(first.min(lines.len())),
        thousands(last),
        thousands(lines.len())
    ))];
    if state.pager.shift > 0 {
        right.push(Span::styled(
            format!("  \u{2192}{}", state.pager.shift),
            theme.dim(),
        ));
    }
    right.push(Span::raw(" "));

    justify(left, right, width).style(theme.bar())
}

/// The visible slice of the field, with fenced blocks tinted.
pub fn body(state: &State, table: &Table, view: Viewport, theme: Theme) -> Vec<Line<'static>> {
    let lines = pager_view(state, table, view);
    let raw = state.pager_text(table);
    let segments = if has_fence(raw) {
        classify(&lines)
    } else if looks_structured(raw) {
        // A nested JSON value: tint the whole field rather than hunting for
        // fences it will never contain.
        vec![Segment::Code; lines.len()]
    } else {
        // Plain prose, which is most fields; skip the walk entirely.
        vec![Segment::Text; lines.len()]
    };

    lines
        .iter()
        .zip(segments)
        .skip(state.pager.scroll)
        .take(view.height)
        .map(|(text, segment)| {
            let style = match segment {
                Segment::Text => theme.text(),
                Segment::Fence => theme.fence(),
                Segment::Code => theme.code(),
            };
            Line::from(Span::styled(text.clone(), style))
        })
        .collect()
}
