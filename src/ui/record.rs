//! The record view: one row per screen, fields stacked vertically.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::data::Table;
use crate::layout::{BodyLine, LineRole};
use crate::state::{State, Viewport, body_for};
use crate::ui::{Theme, footer, justify, split, thousands};

/// Width of the selection gutter, including the trailing space.
pub const GUTTER: usize = 2;

const HINTS: &str =
    "h/l row  j/k field  ^d/^u scroll  z expand  \u{21b5} open  / search  ? help  q quit";

/// Draw the record view.
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

    frame.render_widget(status_bar(state, table, theme, width), status_area);
    frame.render_widget(
        Paragraph::new(body(state, table, view, theme, width)),
        body_area,
    );
    frame.render_widget(footer(state, theme, width, HINTS), footer_area);
}

/// The top bar: file name, position, load warnings and current mode.
pub fn status_bar(state: &State, table: &Table, theme: Theme, width: usize) -> Line<'static> {
    let left = vec![Span::raw(format!(" {}", table.name))];

    let mut right = Vec::new();
    if table.is_empty() {
        right.push(Span::raw("no rows"));
    } else {
        right.push(Span::raw(format!(
            "row {}/{}",
            thousands(state.row + 1),
            thousands(table.len())
        )));
    }
    if !table.warnings.is_empty() {
        right.push(Span::raw("   "));
        right.push(Span::styled(
            format!("\u{26a0} {} malformed", thousands(table.warnings.len())),
            theme.warning(),
        ));
    }
    right.push(Span::raw("   RECORD "));

    justify(left, right, width).style(theme.bar())
}

/// The visible slice of the record body.
pub fn body(
    state: &State,
    table: &Table,
    view: Viewport,
    theme: Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let lines = body_for(state, table, view);
    let content_width = width.saturating_sub(GUTTER);

    lines
        .iter()
        .skip(state.scroll)
        .take(view.height)
        .map(|line| render_line(line, state, table.width(), theme, content_width))
        .collect()
}

/// Render one body line, including its selection gutter.
fn render_line(
    line: &BodyLine,
    state: &State,
    fields: usize,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let selected = line.field == state.field && line.selectable();
    let gutter = if selected {
        Span::styled("\u{258c} ", theme.marker())
    } else {
        Span::raw("  ")
    };

    let rest = match &line.role {
        LineRole::Header { total, shown } => {
            let style = if selected {
                theme.header_selected()
            } else {
                theme.header()
            };
            let mut right = vec![Span::styled(
                format!("{}/{}", line.field + 1, fields),
                theme.dim(),
            )];
            if total > shown {
                right.insert(
                    0,
                    Span::styled(
                        format!("1-{} of {}   ", thousands(*shown), thousands(*total)),
                        theme.dim(),
                    ),
                );
            }
            let composed = justify(vec![Span::styled(line.text.clone(), style)], right, width);
            return prepend(gutter, composed);
        }
        LineRole::Content => Span::styled(line.text.clone(), theme.text()),
        LineRole::Placeholder => Span::styled(line.text.clone(), theme.placeholder()),
        LineRole::More { hidden } => Span::styled(
            format!(
                "\u{22ef} {} more lines \u{b7} z expand \u{b7} \u{21b5} open",
                thousands(*hidden)
            ),
            theme.dim(),
        ),
        LineRole::Blank => Span::raw(""),
    };

    Line::from(vec![gutter, rest])
}

/// Put the gutter in front of an already-composed line.
fn prepend(gutter: Span<'static>, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![gutter];
    spans.extend(line.spans);
    Line::from(spans)
}
