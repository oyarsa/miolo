//! View state and the pure transition function driving it.
//!
//! [`update`] is the only place state changes. It takes the current state, the
//! table, the viewport geometry and an action, and returns the next state —
//! no interior mutation, no side effects, no terminal access.

use crate::data::Table;
use crate::help;
use crate::layout::{
    BodyLine, build_body, clamp_scroll, longest_line, pager_lines, scroll_to_show,
};
use crate::search;

/// Which view has the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Record,
    Pager,
    Table,
    Help,
}

/// Rows of chrome above the table body: status bar plus the column header.
pub const TABLE_CHROME: usize = 2;
/// Rows of chrome above the record body: the status bar.
pub const RECORD_CHROME: usize = 1;

/// Pager position within a single field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pager {
    /// Field being read.
    pub field: usize,
    /// First visible line.
    pub scroll: usize,
    /// Horizontal offset in cells; non-zero chops rather than wraps.
    pub shift: usize,
}

/// Which kind of input the prompt is collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    Search,
    Jump,
}

impl Prompt {
    pub fn sigil(self) -> char {
        match self {
            Self::Search => '/',
            Self::Jump => ':',
        }
    }
}

/// Geometry the transition function needs to clamp scrolling sensibly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Usable width for record field content, excluding the gutter.
    pub width: usize,
    /// Full terminal width, used by the pager and table.
    pub full_width: usize,
    /// Usable height of the body, excluding status and footer bars.
    pub height: usize,
    /// Maximum lines a single field may occupy.
    pub cap: usize,
}

impl Viewport {
    /// Half a page, the unit `^d` and `^u` move by.
    pub fn half_page(self) -> usize {
        (self.height / 2).max(1)
    }

    /// Half a screen width, the unit horizontal scrolling moves by.
    pub fn half_width(self) -> usize {
        (self.full_width / 2).max(1)
    }

    /// Table body height, which loses a row to the column header.
    pub fn table_height(self) -> usize {
        self.height.saturating_sub(1)
    }
}

/// Everything the UI needs to render, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub mode: Mode,
    /// Mode to return to when the help overlay closes.
    pub help_return: Mode,
    /// Selected data row, zero-based.
    pub row: usize,
    /// Selected field within the row.
    pub field: usize,
    /// First body line visible in the record view.
    pub scroll: usize,
    /// Field currently expanded past its cap, if any.
    pub expanded: Option<usize>,
    /// False means truncate rather than wrap.
    pub wrap: bool,
    pub pager: Pager,
    /// First row visible in the table view.
    pub table_top: usize,
    /// First data column visible in the table view.
    pub column_offset: usize,
    /// Column-name search term; persists across rows.
    pub column_term: String,
    /// Field-content search term, used inside the pager.
    pub content_term: String,
    /// First visible line of the help overlay.
    pub help_scroll: usize,
    /// In-progress prompt, if the user is typing one.
    pub prompt: Option<(Prompt, String)>,
    /// Transient message shown in the footer.
    pub status: Option<String>,
    pub quit: bool,
}

impl State {
    pub fn new(wrap: bool) -> Self {
        Self {
            mode: Mode::Record,
            help_return: Mode::Record,
            row: 0,
            field: 0,
            scroll: 0,
            expanded: None,
            wrap,
            pager: Pager::default(),
            table_top: 0,
            column_offset: 0,
            column_term: String::new(),
            content_term: String::new(),
            help_scroll: 0,
            prompt: None,
            status: None,
            quit: false,
        }
    }

    /// Raw text of the field the pager is showing.
    pub fn pager_text<'a>(&self, table: &'a Table) -> &'a str {
        table.field(self.row, self.pager.field)
    }
}

/// A single user intent, already decoded from a key or mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Left,
    Right,
    ColumnLeft,
    ColumnRight,
    HalfDown,
    HalfUp,
    First,
    Last,
    ScrollDown(usize),
    ScrollUp(usize),
    Enter,
    Back,
    ToggleTable,
    ToggleExpand,
    ToggleWrap,
    ToggleHelp,
    BeginSearch,
    BeginJump,
    PromptPush(char),
    PromptPop,
    PromptSubmit,
    PromptCancel,
    NextMatch,
    PrevMatch,
    Yank,
    Quit,
    /// Screen row that was clicked, zero-based from the top of the frame.
    Click(usize),
}

/// Build the current row's body lines. Shared by the transition function and
/// the renderer so both agree on what is on screen.
pub fn body_for(state: &State, table: &Table, view: Viewport) -> Vec<BodyLine> {
    let empty = Vec::new();
    let row = table.rows.get(state.row).unwrap_or(&empty);
    build_body(
        &table.headers,
        row,
        view.width,
        view.cap,
        state.expanded,
        state.wrap,
    )
}

/// Display lines for the pager, honouring wrap and horizontal shift.
pub fn pager_view(state: &State, table: &Table, view: Viewport) -> Vec<String> {
    pager_lines(
        state.pager_text(table),
        view.full_width,
        state.pager.shift,
        state.wrap,
    )
}

/// Apply one action, returning the next state.
pub fn update(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = State {
        status: None,
        ..state.clone()
    };

    if next.prompt.is_some() {
        return prompt_action(&next, table, view, action);
    }

    match action {
        Action::Quit => next.quit = true,
        Action::ToggleHelp => {
            if next.mode == Mode::Help {
                next.mode = next.help_return;
            } else {
                next.help_return = next.mode;
                next.mode = Mode::Help;
                next.help_scroll = 0;
            }
        }
        Action::ToggleWrap => {
            next.wrap = !next.wrap;
            // Turning wrapping back on has nothing to shift against.
            if next.wrap {
                next.pager.shift = 0;
            }
            next.status = Some(if next.wrap { "Wrap" } else { "Truncate" }.to_owned());
        }
        Action::BeginSearch => next.prompt = Some((Prompt::Search, String::new())),
        Action::BeginJump => next.prompt = Some((Prompt::Jump, String::new())),
        Action::Yank => {} // Performed by the shell; state is untouched.
        _ => {
            next = match next.mode {
                Mode::Record => record_action(&next, table, view, action),
                Mode::Pager => pager_action(&next, table, view, action),
                Mode::Table => table_action(&next, table, view, action),
                Mode::Help => help_action(&next, table, view, action),
            };
        }
    }

    reconcile(&next, table, view, action)
}

/// Record view transitions.
fn record_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let last_row = table.len().saturating_sub(1);
    let last_field = table.width().saturating_sub(1);

    match action {
        Action::Down => next.field = (state.field + 1).min(last_field),
        Action::Up => next.field = state.field.saturating_sub(1),
        Action::First => next.field = 0,
        Action::Last => next.field = last_field,

        Action::Left | Action::Right => {
            let target = if action == Action::Right {
                (state.row + 1).min(last_row)
            } else {
                state.row.saturating_sub(1)
            };
            if target == state.row && !table.is_empty() {
                next.status = Some(edge_message(action == Action::Right).to_owned());
            }
            next.row = target;
            // A field expanded on one row should not stay expanded on the
            // next, where the same column may be a single line.
            next.expanded = None;
        }

        Action::HalfDown => next.scroll = state.scroll + view.half_page(),
        Action::HalfUp => next.scroll = state.scroll.saturating_sub(view.half_page()),
        Action::ScrollDown(n) => next.scroll = state.scroll + n,
        Action::ScrollUp(n) => next.scroll = state.scroll.saturating_sub(n),

        Action::ToggleExpand => {
            next.expanded = if state.expanded == Some(state.field) {
                None
            } else {
                Some(state.field)
            };
        }

        Action::Enter => {
            next.mode = Mode::Pager;
            next.pager = Pager {
                field: state.field,
                scroll: 0,
                shift: 0,
            };
        }

        Action::ToggleTable => {
            next.mode = Mode::Table;
        }

        Action::NextMatch | Action::PrevMatch => {
            next = step_column(&next, table, action == Action::NextMatch);
        }

        Action::Click(y) => {
            if let Some(field) = field_at(state, table, view, y) {
                next.field = field;
            }
        }

        _ => {}
    }
    next
}

/// Pager transitions.
fn pager_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let lines = pager_view(state, table, view);
    let last = lines.len().saturating_sub(1);
    let widest = longest_line(state.pager_text(table));

    match action {
        Action::Down => next.pager.scroll = (state.pager.scroll + 1).min(last),
        Action::Up => next.pager.scroll = state.pager.scroll.saturating_sub(1),
        Action::ScrollDown(n) => next.pager.scroll = (state.pager.scroll + n).min(last),
        Action::ScrollUp(n) => next.pager.scroll = state.pager.scroll.saturating_sub(n),
        Action::HalfDown => next.pager.scroll = (state.pager.scroll + view.half_page()).min(last),
        Action::HalfUp => next.pager.scroll = state.pager.scroll.saturating_sub(view.half_page()),
        Action::First => next.pager.scroll = 0,
        Action::Last => next.pager.scroll = last.saturating_sub(view.height.saturating_sub(1)),

        // Horizontal scrolling follows less: shifting chops, and returning to
        // zero restores wrapping.
        Action::Left => next.pager.shift = state.pager.shift.saturating_sub(view.half_width()),
        Action::Right => {
            let limit = widest.saturating_sub(view.half_width());
            next.pager.shift = (state.pager.shift + view.half_width()).min(limit);
        }

        Action::Back | Action::Enter => next.mode = Mode::Record,
        Action::ToggleTable => next.mode = Mode::Table,

        Action::NextMatch | Action::PrevMatch => {
            let matches = search::find(&lines, &state.content_term);
            if let Some(target) =
                search::step(&matches, state.pager.scroll, action == Action::NextMatch)
            {
                next.pager.scroll = target;
            } else if !state.content_term.is_empty() {
                next.status = Some(format!("No match: {}", state.content_term));
            }
        }

        _ => {}
    }

    next.pager.scroll = clamp_scroll(next.pager.scroll, lines.len(), view.height);
    next
}

/// Table view transitions.
fn table_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let last_row = table.len().saturating_sub(1);
    let last_column = table.width().saturating_sub(1);

    match action {
        Action::Down => next.row = (state.row + 1).min(last_row),
        Action::Up => next.row = state.row.saturating_sub(1),
        Action::ScrollDown(n) => next.row = (state.row + n).min(last_row),
        Action::ScrollUp(n) => next.row = state.row.saturating_sub(n),
        Action::HalfDown => next.row = (state.row + view.half_page()).min(last_row),
        Action::HalfUp => next.row = state.row.saturating_sub(view.half_page()),
        Action::First => next.row = 0,
        Action::Last => next.row = last_row,

        Action::Left | Action::ColumnLeft => {
            next.column_offset = state.column_offset.saturating_sub(1);
        }
        Action::Right | Action::ColumnRight => {
            next.column_offset = (state.column_offset + 1).min(last_column);
        }

        Action::Enter | Action::ToggleTable | Action::Back => next.mode = Mode::Record,

        Action::NextMatch | Action::PrevMatch => {
            next = step_column(&next, table, action == Action::NextMatch);
            next.column_offset = next.field;
        }

        Action::Click(y) if y >= TABLE_CHROME => {
            let index = state.table_top + (y - TABLE_CHROME);
            next.row = index.min(last_row);
        }

        _ => {}
    }
    next
}

/// Help overlay: scrollable, with `Esc` or `Enter` closing it.
fn help_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let total = help::content(table).len();

    match action {
        Action::Back | Action::Enter | Action::ToggleTable => {
            next.mode = next.help_return;
            next.help_scroll = 0;
        }
        Action::Down | Action::ScrollDown(_) => next.help_scroll = state.help_scroll + 1,
        Action::Up | Action::ScrollUp(_) => next.help_scroll = state.help_scroll.saturating_sub(1),
        Action::HalfDown => next.help_scroll = state.help_scroll + view.half_page(),
        Action::HalfUp => next.help_scroll = state.help_scroll.saturating_sub(view.half_page()),
        Action::First => next.help_scroll = 0,
        Action::Last => next.help_scroll = total,
        _ => {}
    }

    next.help_scroll = clamp_scroll(next.help_scroll, total, view.height);
    next
}

/// Route a keystroke into the active prompt.
fn prompt_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let Some((kind, mut buffer)) = state.prompt.clone() else {
        return next;
    };

    match action {
        Action::PromptPush(c) => {
            buffer.push(c);
            next.prompt = Some((kind, buffer));
        }
        Action::PromptPop => {
            buffer.pop();
            if buffer.is_empty() {
                next.prompt = None;
            } else {
                next.prompt = Some((kind, buffer));
            }
        }
        Action::PromptCancel | Action::Back => next.prompt = None,
        Action::PromptSubmit => {
            next.prompt = None;
            next = submit(&next, table, view, kind, &buffer);
        }
        _ => {}
    }
    next
}

/// Act on a completed prompt.
fn submit(state: &State, table: &Table, view: Viewport, kind: Prompt, buffer: &str) -> State {
    let mut next = state.clone();
    match kind {
        Prompt::Jump => {
            let target = if buffer == "$" {
                Some(table.len().saturating_sub(1))
            } else {
                buffer.trim().parse::<usize>().ok().and_then(|n| {
                    // Rows are one-based on screen.
                    (n >= 1 && n <= table.len()).then_some(n - 1)
                })
            };
            match target {
                Some(row) => {
                    next.row = row;
                    next.expanded = None;
                }
                None => next.status = Some(format!("No such row: {buffer}")),
            }
        }
        Prompt::Search => {
            if state.mode == Mode::Pager {
                buffer.clone_into(&mut next.content_term);
                let lines = pager_view(state, table, view);
                let matches = search::find(&lines, buffer);
                match search::first_from(&matches, state.pager.scroll) {
                    Some(line) => next.pager.scroll = line,
                    None => next.status = Some(format!("No match: {buffer}")),
                }
            } else {
                buffer.clone_into(&mut next.column_term);
                let matches = search::find(&table.headers, buffer);
                match search::first_from(&matches, state.field) {
                    Some(field) => {
                        next.field = field;
                        if next.mode == Mode::Table {
                            next.column_offset = field;
                        }
                    }
                    None => next.status = Some(format!("No match: {buffer}")),
                }
            }
        }
    }
    next
}

/// Move the field selection to the next column-name match.
fn step_column(state: &State, table: &Table, forward: bool) -> State {
    let mut next = state.clone();
    let matches = search::find(&table.headers, &state.column_term);
    if let Some(field) = search::step(&matches, state.field, forward) {
        next.field = field;
    } else if state.column_term.is_empty() {
        next.status = Some("No search term".to_owned());
    } else {
        next.status = Some(format!("No match: {}", state.column_term));
    }
    next
}

/// Which field a click landed on, if any.
fn field_at(state: &State, table: &Table, view: Viewport, y: usize) -> Option<usize> {
    if y < RECORD_CHROME {
        return None;
    }
    let body = body_for(state, table, view);
    let index = state.scroll + (y - RECORD_CHROME);
    body.get(index).filter(|l| l.selectable()).map(|l| l.field)
}

fn edge_message(forward: bool) -> &'static str {
    if forward { "Last row" } else { "First row" }
}

/// Keep scroll and selection consistent after a change.
///
/// Moving the selection pulls the viewport to it; scrolling explicitly is
/// allowed to move the viewport away from the selection, matching how a pager
/// behaves.
fn reconcile(next: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut out = next.clone();

    if out.mode == Mode::Record {
        let body = body_for(&out, table, view);
        let follows_selection = !matches!(
            action,
            Action::HalfDown | Action::HalfUp | Action::ScrollDown(_) | Action::ScrollUp(_)
        );
        out.scroll = if follows_selection {
            scroll_to_show(&body, out.field, out.scroll, view.height)
        } else {
            clamp_scroll(out.scroll, body.len(), view.height)
        };
    }

    if out.mode == Mode::Table {
        let height = view.table_height();
        if out.row < out.table_top {
            out.table_top = out.row;
        } else if out.row >= out.table_top + height.max(1) {
            out.table_top = out.row + 1 - height.max(1);
        }
        out.table_top = clamp_scroll(out.table_top, table.len(), height);
    }

    out
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
