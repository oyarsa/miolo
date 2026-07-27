//! Rendering for the help overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::data::Table;
use crate::help::{HelpLine, content};
use crate::state::{State, Viewport};
use crate::ui::Theme;

/// Draw the overlay over the whole screen.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    table: &Table,
    view: Viewport,
    theme: Theme,
) {
    let total = content(table).len();
    let shown = usize::from(area.height).saturating_sub(2);
    let more = total.saturating_sub(state.help_scroll + shown);
    let title = if more > 0 {
        format!(" miolo \u{2014} j/k to scroll, {more} more lines below ")
    } else {
        " miolo \u{2014} press ? or Esc to close ".to_owned()
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines(state, table, view, theme)), inner);
}

/// The visible slice of the overlay.
pub fn lines(state: &State, table: &Table, _view: Viewport, theme: Theme) -> Vec<Line<'static>> {
    content(table)
        .into_iter()
        .skip(state.help_scroll)
        .map(|line| match line {
            HelpLine::Section(title) => Line::from(Span::styled(title.to_owned(), theme.header())),
            HelpLine::Binding { keys, text } => Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{keys:<8}"), theme.header_selected()),
                Span::raw(text.to_owned()),
            ]),
            HelpLine::Blank => Line::raw(""),
            HelpLine::Warning { row, text } => Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("row {row:<6}"), theme.dim()),
                Span::styled(text, theme.warning()),
            ]),
            HelpLine::Note(text) => Line::from(Span::styled(format!("  {text}"), theme.dim())),
        })
        .collect()
}
