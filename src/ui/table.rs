//! The table view: many rows at a glance, for finding one to read.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::data::Table;
use crate::layout::{display_width, one_line, truncate_to_width};
use crate::state::{State, Viewport};
use crate::ui::{Context, Theme, footer, justify, split, thousands};

const HINTS: &str = "\u{21b5} open record  j/k row  H/L column  / search  :N jump  ? help";

/// Rows sampled to decide column widths. Sampling rather than scanning keeps
/// startup cheap on large files.
const SAMPLE_ROWS: usize = 1000;
/// Widest a single column may become.
const MAX_COLUMN: usize = 32;
/// Narrowest a column may be squeezed to before it is simply dropped.
const MIN_COLUMN: usize = 6;
/// Gap between columns.
const GAP: usize = 2;

/// Draw the table.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    table: &Table,
    view: Viewport,
    ctx: &Context,
) {
    let [status_area, body_area, footer_area] = split(area);
    let width = area.width as usize;
    let theme = ctx.theme;

    frame.render_widget(status_bar(state, table, view, theme, width), status_area);
    frame.render_widget(
        Paragraph::new(body(state, table, view, ctx, width)),
        body_area,
    );
    frame.render_widget(footer(state, theme, width, HINTS), footer_area);
}

/// Width of the pinned row-number column.
fn gutter_width(table: &Table) -> usize {
    thousands(table.len().max(1)).len().max(2) + 1
}

/// Width to give each data column, sampled from the first rows.
pub fn column_widths(table: &Table) -> Vec<usize> {
    table
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let widest = table
                .rows
                .iter()
                .take(SAMPLE_ROWS)
                .map(|row| {
                    row.get(index)
                        .map_or(0, |cell| display_width(&one_line(cell, MAX_COLUMN)))
                })
                .max()
                .unwrap_or(0);
            widest
                .max(display_width(header))
                .clamp(MIN_COLUMN, MAX_COLUMN)
        })
        .collect()
}

/// Which columns fit, starting from the current horizontal offset.
fn visible_columns(table: &Table, widths: &[usize], offset: usize, width: usize) -> Vec<usize> {
    let mut used = gutter_width(table);
    let mut columns = Vec::new();
    for index in offset..table.width() {
        let needed = widths.get(index).copied().unwrap_or(MIN_COLUMN) + GAP;
        if used + needed > width && !columns.is_empty() {
            break;
        }
        used += needed;
        columns.push(index);
    }
    columns
}

/// `file    rows 38-44/1,337    TABLE`
pub fn status_bar(
    state: &State,
    table: &Table,
    view: Viewport,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let left = vec![Span::raw(format!(" {}", table.name))];
    let height = view.table_height();
    let last = (state.table_top + height).min(table.len());

    let mut right = Vec::new();
    if table.is_empty() {
        right.push(Span::raw("no rows"));
    } else {
        right.push(Span::raw(format!(
            "rows {}-{}/{}",
            thousands(state.table_top + 1),
            thousands(last),
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
    right.push(Span::raw("   TABLE "));

    justify(left, right, width).style(theme.bar())
}

/// Column header row followed by the visible data rows.
pub fn body(
    state: &State,
    table: &Table,
    view: Viewport,
    ctx: &Context,
    width: usize,
) -> Vec<Line<'static>> {
    let widths = &ctx.widths;
    let theme = ctx.theme;
    let columns = visible_columns(table, widths, state.column_offset, width);
    let gutter = gutter_width(table);

    let mut lines = vec![header_row(table, widths, &columns, gutter, theme)];
    for offset in 0..view.table_height() {
        let index = state.table_top + offset;
        if index >= table.len() {
            break;
        }
        lines.push(data_row(
            state, table, widths, &columns, gutter, index, theme,
        ));
    }
    lines
}

fn header_row(
    table: &Table,
    widths: &[usize],
    columns: &[usize],
    gutter: usize,
    theme: Theme,
) -> Line<'static> {
    let mut spans = vec![Span::styled(pad("#", gutter), theme.dim())];
    for &index in columns {
        let width = widths.get(index).copied().unwrap_or(MIN_COLUMN);
        let name = table.headers.get(index).cloned().unwrap_or_default();
        spans.push(Span::styled(
            pad(&truncate_to_width(&name, width), width + GAP),
            theme.header(),
        ));
    }
    Line::from(spans)
}

fn data_row(
    state: &State,
    table: &Table,
    widths: &[usize],
    columns: &[usize],
    gutter: usize,
    index: usize,
    theme: Theme,
) -> Line<'static> {
    let selected = index == state.row;
    let marker = if selected {
        Span::styled("\u{258c}", theme.marker())
    } else {
        Span::raw(" ")
    };

    let mut spans = vec![
        marker,
        Span::styled(pad(&thousands(index + 1), gutter - 1), theme.dim()),
    ];
    for &column in columns {
        let width = widths.get(column).copied().unwrap_or(MIN_COLUMN);
        let cell = one_line(table.field(index, column), width);
        let style = if selected {
            theme.header_selected()
        } else {
            theme.text()
        };
        spans.push(Span::styled(pad(&cell, width + GAP), style));
    }
    Line::from(spans)
}

/// Pad to an exact display width, accounting for wide characters.
fn pad(text: &str, width: usize) -> String {
    let used = display_width(text);
    let mut out = text.to_owned();
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;

    fn sample() -> Table {
        parse(b"id,notes\n1,short\n2,\"a\nb\"\n", b',', "test").expect("parse failed")
    }

    #[test]
    fn column_widths_respect_the_bounds() {
        let widths = column_widths(&sample());
        assert!(widths.iter().all(|w| (MIN_COLUMN..=MAX_COLUMN).contains(w)));
    }

    #[test]
    fn column_widths_cover_the_header() {
        let table = parse(b"a_very_long_header_name\nx\n", b',', "t").expect("parse failed");
        let widths = column_widths(&table);
        assert!(widths[0] >= MIN_COLUMN);
    }

    #[test]
    fn visible_columns_stop_at_the_edge() {
        let table = sample();
        let widths = column_widths(&table);
        let columns = visible_columns(&table, &widths, 0, 20);
        assert!(!columns.is_empty(), "always shows at least one column");
        assert!(columns.len() <= table.width());
    }

    #[test]
    fn visible_columns_start_from_the_offset() {
        let table = sample();
        let widths = column_widths(&table);
        let columns = visible_columns(&table, &widths, 1, 200);
        assert_eq!(columns.first(), Some(&1));
    }

    #[test]
    fn padding_is_width_aware() {
        assert_eq!(display_width(&pad("ab", 5)), 5);
        assert_eq!(display_width(&pad("日本", 6)), 6);
        assert_eq!(pad("toolong", 3), "toolong", "never truncates");
    }
}
