//! Rendering. Draws from state and decides nothing.
//!
//! Anything that requires a decision — what is selected, where the viewport
//! sits, how text wraps — belongs in `state.rs` or `layout.rs`.

pub mod edit;
pub mod help;
pub mod pager;
pub mod record;
pub mod table;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::data::{Table, write};
use crate::layout::{display_width, truncate_to_width};
use crate::state::{Mode, State, Viewport};

/// Colours come from the terminal's 16 ANSI slots so the viewer follows the
/// user's theme rather than imposing one.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub color: bool,
}

impl Theme {
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    /// Whether colour is wanted, from the flag and the environment.
    ///
    /// `NO_COLOR` is the user declining it. `TERM=dumb` is the terminal saying
    /// it has none to give, which amounts to the same answer here but for a
    /// different reason: crossterm writes ANSI without consulting terminfo, so
    /// this is the only place that can decline on the terminal's behalf.
    pub fn wanted(flag: bool, no_color: bool, term: Option<&str>) -> bool {
        !flag && !no_color && term != Some("dumb")
    }

    fn tinted(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    /// The status and footer bars.
    #[allow(
        clippy::unused_self,
        reason = "uniform interface with the tinted styles"
    )]
    pub fn bar(self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// A column name.
    pub fn header(self) -> Style {
        self.tinted(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    /// The column name of the selected field.
    pub fn header_selected(self) -> Style {
        self.tinted(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    /// Gutter marker against the selection.
    pub fn marker(self) -> Style {
        self.tinted(Color::Yellow)
    }

    /// Counters, `⋯ N more lines`, and other chrome.
    #[allow(
        clippy::unused_self,
        reason = "uniform interface with the tinted styles"
    )]
    pub fn dim(self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// `(empty)` and `(whitespace)` stand-ins.
    pub fn placeholder(self) -> Style {
        self.tinted(Color::Blue).add_modifier(Modifier::DIM)
    }

    /// Load warnings.
    pub fn warning(self) -> Style {
        self.tinted(Color::Red)
    }

    /// A fence marker line.
    pub fn fence(self) -> Style {
        self.tinted(Color::Magenta).add_modifier(Modifier::DIM)
    }

    /// A line inside a fenced block.
    pub fn code(self) -> Style {
        self.tinted(Color::Green)
    }

    /// Ordinary field text.
    #[allow(
        clippy::unused_self,
        reason = "uniform interface with the tinted styles"
    )]
    pub fn text(self) -> Style {
        Style::default()
    }
}

/// Everything the renderer needs that does not change between frames.
///
/// Column widths are sampled from the data and cost real time on a large file,
/// so they are computed once at startup rather than per frame. They depend
/// only on the table, not on the terminal size, so a resize does not
/// invalidate them.
pub struct Context {
    pub theme: Theme,
    pub widths: Vec<usize>,
}

impl Context {
    pub fn new(theme: Theme, table: &Table) -> Self {
        Self {
            theme,
            widths: table::column_widths(table),
        }
    }

    /// Re-sample one column after an edit changed what is in it.
    ///
    /// Only the edited column, because re-sampling all of them is the cost
    /// that made this a cached value in the first place.
    pub fn refresh_column(&mut self, table: &Table, col: usize) {
        if let Some(width) = self.widths.get_mut(col) {
            *width = table::column_width(table, col);
        }
    }
}

/// Draw whichever view is active.
pub fn render(frame: &mut Frame, state: &State, table: &Table, view: Viewport, ctx: &Context) {
    let area = frame.area();
    let theme = ctx.theme;
    match state.mode {
        Mode::Record => record::render(frame, area, state, table, view, theme),
        Mode::Pager => pager::render(frame, area, state, table, view, theme),
        Mode::Table => table::render(frame, area, state, table, view, ctx),
        Mode::Help => help::render(frame, area, state, table, view, theme),
        // An inline edit is the record view with the buffer spliced into it,
        // so it is drawn by the record view rather than by a second one.
        Mode::Edit => match state.editing.as_ref().map(|e| e.surface) {
            Some(crate::edit::Surface::FullScreen) => {
                edit::render(frame, area, state, table, view, theme);
            }
            _ => record::render(frame, area, state, table, view, theme),
        },
    }
}

/// Marker for a table with edits that are not on disk yet.
///
/// Shown in every view's status bar, because the one thing worse than an
/// unsaved change is not knowing you have one.
pub fn unsaved(table: &Table, theme: Theme) -> Vec<Span<'static>> {
    if table.dirty {
        vec![
            Span::raw("   "),
            Span::styled("\u{25cf} unsaved", theme.marker()),
        ]
    } else {
        Vec::new()
    }
}

/// Split a view into status bar, body and footer.
pub fn split(area: Rect) -> [Rect; 3] {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

/// The bottom bar: an open prompt wins, then a transient message, then hints.
pub fn footer(
    state: &State,
    table: &Table,
    theme: Theme,
    width: usize,
    hints: &str,
) -> Line<'static> {
    if let Some((kind, buffer)) = &state.prompt {
        let text = format!(" {}{buffer}", kind.sigil());
        // A block cursor, so it is obvious the prompt is taking input.
        return Line::from(vec![Span::raw(text), Span::styled("\u{2588}", theme.dim())])
            .style(theme.bar());
    }

    if let Some(status) = &state.status {
        // Clip with an ellipsis rather than letting the buffer cut mid-word.
        let text = truncate_to_width(status, width.saturating_sub(1));
        return justify(vec![Span::raw(format!(" {text}"))], Vec::new(), width).style(theme.bar());
    }

    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    // `W` earns a place on the bar only while pressing it would do something.
    // It goes first, in the colour of the `● unsaved` marker it answers,
    // because the hints are already long enough to be clipped.
    if writable_edits(table) {
        let save = "W save  ";
        spans.push(Span::styled(save, theme.marker()));
        used += display_width(save);
    }
    spans.push(Span::raw(truncate_to_width(
        hints,
        width.saturating_sub(used + 1),
    )));
    justify(spans, Vec::new(), width).style(theme.bar())
}

/// Whether there are changes and somewhere to write them.
fn writable_edits(table: &Table) -> bool {
    table.dirty && write::blocker(&table.origin).is_none()
}

/// Compose a line with `left` flush left and `right` flush right.
///
/// When the two would collide, the left side is truncated — a row counter you
/// cannot read is worse than a column name you can only half read.
pub fn justify(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let left_width: usize = left.iter().map(|s| display_width(&s.content)).sum();
    let right_width: usize = right.iter().map(|s| display_width(&s.content)).sum();

    let mut spans = left;
    if left_width + right_width >= width {
        spans.extend(right);
        return Line::from(spans);
    }
    spans.push(Span::raw(" ".repeat(width - left_width - right_width)));
    spans.extend(right);
    Line::from(spans)
}

/// Format a count with thousands separators.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
#[path = "tests.rs"]
mod render_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn colour_is_on_unless_something_declines_it() {
        assert!(Theme::wanted(false, false, Some("xterm-256color")));
        assert!(Theme::wanted(false, false, None), "TERM may be unset");

        assert!(!Theme::wanted(true, false, Some("xterm")), "--no-color");
        assert!(!Theme::wanted(false, true, Some("xterm")), "NO_COLOR");
        assert!(
            !Theme::wanted(false, false, Some("dumb")),
            "a dumb terminal has no colour to give"
        );
    }

    #[test]
    fn thousands_separates_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(42), "42");
        assert_eq!(thousands(1337), "1,337");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn justify_pads_between_the_sides() {
        let line = justify(vec![Span::raw("ab")], vec![Span::raw("cd")], 10);
        assert_eq!(rendered(&line), "ab      cd");
        assert_eq!(display_width(&rendered(&line)), 10);
    }

    #[test]
    fn justify_does_not_pad_when_the_sides_collide() {
        let line = justify(vec![Span::raw("aaaa")], vec![Span::raw("bbbb")], 6);
        assert_eq!(rendered(&line), "aaaabbbb");
    }

    /// A table with somewhere to write back to, and unsaved changes in it.
    fn edited() -> Table {
        let mut table = crate::data::parse_csv(b"a,b\n1,2\n", "t").expect("parse failed");
        table.origin.path = Some(std::path::PathBuf::from("/tmp/t.csv"));
        table.dirty = true;
        table
    }

    fn bar(state: &State, table: &Table) -> String {
        rendered(&footer(state, table, Theme::new(false), 40, "hints"))
    }

    #[test]
    fn footer_shows_an_open_prompt() {
        let mut state = State::new(true);
        state.prompt = Some((crate::state::Prompt::Search, "ship".to_owned()));
        assert!(bar(&state, &Table::default()).starts_with(" /ship"));
    }

    #[test]
    fn footer_prefers_a_status_over_hints() {
        let mut state = State::new(true);
        state.status = Some("Last row".to_owned());
        let line = bar(&state, &Table::default());
        assert!(line.contains("Last row"));
        assert!(!line.contains("hints"));
    }

    #[test]
    fn footer_falls_back_to_hints() {
        assert!(bar(&State::new(true), &Table::default()).contains("hints"));
    }

    #[test]
    fn footer_offers_to_write_while_there_is_something_to_write() {
        let line = bar(&State::new(true), &edited());
        assert!(line.contains("W save"), "{line}");
        assert!(line.contains("hints"), "and still the ordinary hints");
    }

    #[test]
    fn footer_stays_quiet_about_writing_when_it_would_do_nothing() {
        let clean = Table {
            dirty: false,
            ..edited()
        };
        assert!(!bar(&State::new(true), &clean).contains("W save"));

        // Edits to something that cannot be written back are not worth
        // advertising a key for.
        let unwritable = Table {
            origin: crate::data::Origin::default(),
            ..edited()
        };
        assert!(!bar(&State::new(true), &unwritable).contains("W save"));
    }

    #[test]
    fn footer_keeps_the_write_hint_when_the_bar_is_too_narrow() {
        let line = rendered(&footer(
            &State::new(true),
            &edited(),
            Theme::new(false),
            16,
            "hints that run well past the edge",
        ));
        assert!(line.contains("W save"), "clipping takes the hints, not it");
        assert!(display_width(&line) <= 16);
    }
}
