//! View state and the pure transition function driving it.
//!
//! [`update`] is the only place state changes. It takes the current state, the
//! table, the viewport geometry and an action, and returns the next state —
//! no interior mutation, no side effects, no terminal access.

use crate::data::{Table, write};
use crate::edit::{self, Editing, Surface};
use crate::help;
use crate::layout::{
    BodyLine, build_body_with, clamp_scroll, field_span, longest_line, pager_lines, scroll_to_line,
    scroll_to_show,
};
use crate::search;

/// Which view has the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Record,
    Pager,
    Table,
    Help,
    Edit,
}

/// What is currently taking keystrokes.
///
/// Key decoding needs this because the same key means different things
/// depending on it: `n` is a search step normally, a letter while editing, and
/// a refusal at a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Normal,
    Prompt,
    Edit,
    Confirm,
}

/// A change to the document that the event loop must carry out.
///
/// The transition function decides *that* something should happen; `main`
/// decides *how*, because the table and the terminal live out there. Keeping
/// the decision in here is what lets every one of these be tested without a
/// filesystem or a clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Replace one field's text.
    Set {
        row: usize,
        col: usize,
        text: String,
    },
    /// Copy the selected field to the clipboard.
    Yank,
    /// Write the table back to its file.
    Save,
    /// Reverse the last committed edit.
    Undo,
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

    /// Width the editor wraps to on a given surface.
    ///
    /// One cell narrower than what is available, so the caret has somewhere to
    /// sit at the end of a row that is otherwise exactly full. Inline, what is
    /// available is the record body, which has already lost the gutter.
    pub fn edit_width(self, surface: Surface) -> usize {
        let width = match surface {
            Surface::Inline => self.width,
            Surface::FullScreen => self.full_width,
        };
        width.saturating_sub(1).max(1)
    }
}

/// Everything the UI needs to render, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub mode: Mode,
    /// Mode to return to when the help overlay closes.
    pub help_return: Mode,
    /// Mode to return to when the editor closes.
    pub edit_return: Mode,
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
    /// Field being edited, if the editor is open.
    pub editing: Option<Editing>,
    /// Change the last action asks the event loop to carry out.
    pub effect: Option<Effect>,
    /// Transient message shown in the footer.
    pub status: Option<String>,
    pub quit: bool,
}

impl State {
    pub fn new(wrap: bool) -> Self {
        Self {
            mode: Mode::Record,
            help_return: Mode::Record,
            edit_return: Mode::Record,
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
            editing: None,
            effect: None,
            status: None,
            quit: false,
        }
    }

    /// Raw text of the field the pager is showing.
    pub fn pager_text<'a>(&self, table: &'a Table) -> &'a str {
        table.field(self.row, self.pager.field)
    }

    /// What should be interpreting keystrokes right now.
    pub fn focus(&self) -> Focus {
        if self.prompt.is_some() {
            Focus::Prompt
        } else if let Some(editing) = &self.editing {
            if editing.confirming {
                Focus::Confirm
            } else {
                Focus::Edit
            }
        } else {
            Focus::Normal
        }
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
    /// Quit without asking about unsaved changes.
    ForceQuit,
    /// Screen row that was clicked, zero-based from the top of the frame.
    Click(usize),

    /// Open the selected field in the editor.
    BeginEdit,
    EditInsert(char),
    EditNewline,
    EditBackspace,
    EditDelete,
    /// Accept the edit and return to the record view.
    EditCommit,
    /// Abandon the edit, asking first if anything was typed.
    EditCancel,
    ConfirmYes,
    ConfirmNo,
    /// Write the table back to its file.
    Save,
    /// Reverse the last committed edit.
    Undo,
    /// Do nothing, but let scrolling settle around whatever changed.
    Refresh,
}

/// Build the current row's body lines. Shared by the transition function and
/// the renderer so both agree on what is on screen.
pub fn body_for(state: &State, table: &Table, view: Viewport) -> Vec<BodyLine> {
    let empty = Vec::new();
    let row = table.rows.get(state.row).unwrap_or(&empty);

    // An inline edit replaces the stored text of one field with the buffer's,
    // so the renderer, the click test and the scroll reconciliation all agree
    // on what is currently on screen.
    let inline = state
        .editing
        .as_ref()
        .filter(|e| e.surface == Surface::Inline);
    let lines = inline.map(|e| e.lines(view.edit_width(Surface::Inline)));

    build_body_with(
        &table.headers,
        row,
        view.width,
        view.cap,
        state.expanded,
        state.wrap,
        inline
            .zip(lines.as_deref())
            .map(|(e, lines)| (e.field, lines)),
    )
}

/// Where the caret sits: the body line it is on, and its column within the
/// record body.
///
/// A decision, not a drawing: the renderer places the terminal cursor from
/// this rather than working it out from the buffer itself.
pub fn caret_in_body(state: &State, table: &Table, view: Viewport) -> Option<(usize, usize)> {
    let editing = state.editing.as_ref()?;
    if editing.surface != Surface::Inline {
        return None;
    }
    let body = body_for(state, table, view);
    // The field's span starts at its header line; content follows it.
    let (header, _) = field_span(&body, editing.field)?;
    let (row, column) = edit::caret_position(&editing.buffer, view.edit_width(Surface::Inline));
    Some((header + 1 + row, column))
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
        effect: None,
        ..state.clone()
    };

    if next.prompt.is_some() {
        return prompt_action(&next, table, view, action);
    }
    // The editor takes every key, so none of the global bindings below can
    // fire while a field is open — `w` is a letter in there, not a toggle.
    //
    // Reconciling on the way out matters: closing the editor puts the height
    // cap back, so the body shrinks, and a scroll offset from deep inside a
    // tall field would be left pointing past the end of it.
    if next.mode == Mode::Edit {
        let next = edit_action(&next, table, view, action);
        return reconcile(&next, table, view, action);
    }

    match action {
        Action::Quit => {
            if table.dirty {
                next.status =
                    Some("Unsaved changes \u{b7} W to write \u{b7} Q to quit anyway".to_owned());
            } else {
                next.quit = true;
            }
        }
        Action::ForceQuit => next.quit = true,
        Action::Save => next.effect = Some(Effect::Save),
        Action::Undo => next.effect = Some(Effect::Undo),
        Action::BeginEdit => next = begin_edit(&next, table, view),
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
        Action::Yank => next.effect = Some(Effect::Yank),
        _ => {
            next = match next.mode {
                Mode::Record => record_action(&next, table, view, action),
                Mode::Pager => pager_action(&next, table, view, action),
                Mode::Table => table_action(&next, table, view, action),
                Mode::Help => help_action(&next, table, view, action),
                Mode::Edit => edit_action(&next, table, view, action),
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

/// Open the selected field in the editor.
///
/// A source that cannot be written back is still editable — the change is
/// useful to yank, and refusing outright would be worse than saying so — but
/// the reason is reported here rather than at the point of saving, when the
/// user has already typed.
fn begin_edit(state: &State, table: &Table, view: Viewport) -> State {
    let mut next = state.clone();
    if state.mode == Mode::Help {
        return next;
    }
    if table.is_empty() || table.width() == 0 {
        next.status = Some("Nothing to edit".to_owned());
        return next;
    }

    // An edit started in the pager stays full-screen, because that is where
    // the field was already being read. Everywhere else edits in place.
    let (field, surface) = match state.mode {
        Mode::Pager => (state.pager.field, Surface::FullScreen),
        _ => (state.field, Surface::Inline),
    };
    next.field = field;
    next.editing = Some(Editing::new(
        state.row,
        field,
        table.field(state.row, field),
        surface,
    ));
    next.edit_return = state.mode;
    next.mode = Mode::Edit;
    next.status = write::blocker(&table.origin)
        .map(|reason| format!("Editing in memory only \u{2014} cannot save: {reason}"));
    // Lifting the cap can move everything below the edited field, so the
    // viewport has to settle before the first keystroke rather than after it.
    follow_caret(&next, table, view)
}

/// Editor transitions.
///
/// The table is read only to place the caret among the *other* fields when
/// editing inline. The text being edited always comes from the buffer, so
/// nothing in here can pick up a field the user has since changed.
fn edit_action(state: &State, table: &Table, view: Viewport, action: Action) -> State {
    let mut next = state.clone();
    let Some(editing) = &state.editing else {
        // No buffer means the editor was never really open; fall back rather
        // than render a mode with nothing in it.
        next.mode = Mode::Record;
        return next;
    };
    let surface = editing.surface;
    let width = view.edit_width(surface);

    // A pending discard prompt swallows everything until it is answered, so a
    // stray keystroke cannot throw away what was typed.
    if editing.confirming {
        match action {
            Action::ConfirmYes => next = close_editor(&next, editing.field),
            Action::ConfirmNo => {
                next.editing = Some(Editing {
                    confirming: false,
                    ..editing.clone()
                });
            }
            _ => {}
        }
        return next;
    }

    let mut edit = editing.clone();
    let half = isize::try_from(view.half_page()).unwrap_or(1);

    match action {
        Action::EditInsert(ch) => edit.buffer = edit::insert(&edit.buffer, ch),
        Action::EditNewline => edit.buffer = edit::insert(&edit.buffer, '\n'),
        Action::EditBackspace => edit.buffer = edit::backspace(&edit.buffer),
        Action::EditDelete => edit.buffer = edit::delete(&edit.buffer),

        Action::Left => edit.buffer = edit::left(&edit.buffer),
        Action::Right => edit.buffer = edit::right(&edit.buffer),
        Action::Up => edit.buffer = edit::move_rows(&edit.buffer, width, -1),
        Action::Down => edit.buffer = edit::move_rows(&edit.buffer, width, 1),
        Action::HalfUp => edit.buffer = edit::move_rows(&edit.buffer, width, -half),
        Action::HalfDown => edit.buffer = edit::move_rows(&edit.buffer, width, half),
        Action::First => edit.buffer = edit::row_start(&edit.buffer, width),
        Action::Last => edit.buffer = edit::row_end(&edit.buffer, width),

        Action::EditCommit => {
            next = close_editor(&next, edit.field);
            if edit.modified() {
                next.effect = Some(Effect::Set {
                    row: edit.row,
                    col: edit.field,
                    text: edit.buffer.text,
                });
            } else {
                next.status = Some("No change".to_owned());
            }
            return next;
        }
        Action::EditCancel | Action::Back => {
            if edit.modified() {
                edit.confirming = true;
            } else {
                return close_editor(&next, edit.field);
            }
        }

        _ => {}
    }

    next.editing = Some(edit);
    follow_caret(&next, table, view)
}

/// Pull the viewport to the caret.
///
/// Inline that means scrolling the record body, which the buffer is only one
/// part of; full-screen it means scrolling the buffer itself. Both the opening
/// of the editor and every keystroke after it come through here, so a field
/// that was only half on screen is not edited blind.
fn follow_caret(state: &State, table: &Table, view: Viewport) -> State {
    let mut next = state.clone();
    let Some(editing) = &state.editing else {
        return next;
    };

    match editing.surface {
        Surface::Inline => {
            let body = body_for(&next, table, view);
            if let Some((line, _)) = caret_in_body(&next, table, view) {
                next.scroll = scroll_to_line(line, next.scroll, body.len(), view.height);
            }
        }
        Surface::FullScreen => {
            let width = view.edit_width(Surface::FullScreen);
            let rows = edit::rows(&editing.buffer.text, width);
            let caret = edit::row_at(&rows, editing.buffer.caret);
            let scroll = scroll_to_line(caret, editing.scroll, rows.len(), view.height);
            next.editing = Some(Editing {
                scroll,
                ..editing.clone()
            });
        }
    }
    next
}

/// Leave the editor for wherever the edit was started, with the edited field
/// selected.
///
/// Going back to the view you came from rather than always to the record view:
/// `e` in the pager is a detour within reading one field, not a way out of it.
///
/// Any expansion the user had set is left alone: editing lifts the cap for the
/// edited field on its own, so there was never any state to put back.
fn close_editor(state: &State, field: usize) -> State {
    State {
        mode: state.edit_return,
        editing: None,
        field,
        ..state.clone()
    }
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
