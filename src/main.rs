//! miolo — a terminal viewer for tabular files with long, multi-line text
//! columns: CSV, TSV, JSON and JSONL, compressed or not.

mod cli;
mod clipboard;
mod data;
mod decompress;
mod edit;
mod help;
mod keys;
mod layout;
mod markdown;
mod search;
mod source;
mod state;
mod ui;

use std::io::{self, Stdout};

use anyhow::{Context, Result, bail};
use clap::Parser;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Size;

use crate::cli::Cli;
use crate::data::{Table, write};
use crate::layout::field_cap;
use crate::state::{Action, Effect, Mode, State, Viewport, update};
use crate::ui::{Theme, record};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // No path means standard input, as for most command-line tools. Reading
    // happens before the terminal is put into raw mode, so a pipe that is
    // still producing simply delays startup.
    let delimiter = cli.delimiter.map(delimiter_byte).transpose()?;
    let table = data::load(cli.file.as_deref(), cli.format, delimiter)?;
    let theme = Theme::new(!cli.no_color && std::env::var_os("NO_COLOR").is_none());

    let mut terminal = setup().context("failed to set up the terminal")?;
    let outcome = run(&mut terminal, table, &cli, theme);
    restore().context("failed to restore the terminal")?;
    outcome
}

/// A delimiter has to be a single byte for the separated-value reader.
fn delimiter_byte(delimiter: char) -> Result<u8> {
    let mut buf = [0u8; 4];
    let encoded = delimiter.encode_utf8(&mut buf);
    if encoded.len() != 1 {
        bail!("delimiter must be a single-byte character, got {delimiter:?}");
    }
    Ok(encoded.as_bytes()[0])
}

/// Geometry for the current terminal size.
fn viewport(size: Size, percent: u8) -> Viewport {
    // Two rows go to the status and footer bars.
    let height = usize::from(size.height).saturating_sub(2);
    let full_width = usize::from(size.width);
    Viewport {
        width: full_width.saturating_sub(record::GUTTER),
        full_width,
        height,
        cap: field_cap(height, percent),
    }
}

type Backend = CrosstermBackend<Stdout>;

fn setup() -> io::Result<Terminal<Backend>> {
    // Restore the terminal even when something panics, so the shell is not
    // left in raw mode with an invisible cursor.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        hook(info);
    }));

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(out))
}

fn restore() -> io::Result<()> {
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
    disable_raw_mode()
}

/// One committed edit, kept so it can be reversed.
struct Change {
    row: usize,
    col: usize,
    /// The text the field held before the edit.
    text: String,
}

fn run(terminal: &mut Terminal<Backend>, mut table: Table, cli: &Cli, theme: Theme) -> Result<()> {
    let mut state = State::new(!cli.no_wrap);
    // Sampling column widths is expensive on a large file, so do it once.
    let mut ctx = ui::Context::new(theme, &table);
    let mut history: Vec<Change> = Vec::new();

    loop {
        let view = viewport(terminal.size()?, cli.max_height);
        terminal.draw(|frame| ui::render(frame, &state, &table, view, &ctx))?;

        let action = match event::read()? {
            Event::Key(key) => keys::action_for(key, state.mode, state.focus()),
            Event::Mouse(mouse) => keys::action_for_mouse(mouse.kind, mouse.row),
            _ => None,
        };

        if let Some(action) = action {
            state = update(&state, &table, view, action);

            // `update` decides what should happen; carrying it out needs the
            // table, the clipboard and the filesystem, none of which belong in
            // a pure function.
            if let Some(effect) = state.effect.take() {
                state = apply(effect, &mut table, &mut ctx, &mut history, state);
                // Settle scrolling around whatever moved, without losing the
                // message describing what just happened.
                let status = state.status.take();
                state = update(&state, &table, view, Action::Refresh);
                state.status = status;
            }
        }

        if state.quit {
            return Ok(());
        }
    }
}

/// Carry out an effect. The only place the table is mutated.
fn apply(
    effect: Effect,
    table: &mut Table,
    ctx: &mut ui::Context,
    history: &mut Vec<Change>,
    state: State,
) -> State {
    match effect {
        Effect::Yank => yank(&state, table),
        Effect::Set { row, col, text } => {
            let previous = table.set_field(row, col, text);
            history.push(Change {
                row,
                col,
                text: previous,
            });
            ctx.refresh_column(table, col);
            table.dirty = true;
            State {
                status: Some(format!("Edited {}", table.column_name(col))),
                ..state
            }
        }
        Effect::Undo => match history.pop() {
            Some(change) => {
                table.set_field(change.row, change.col, change.text);
                ctx.refresh_column(table, change.col);
                // Still dirty even when the stack empties: a save may have
                // happened in between, so the file no longer matches.
                table.dirty = true;
                State {
                    row: change.row,
                    field: change.col,
                    status: Some(format!("Undid edit to {}", table.column_name(change.col))),
                    ..state
                }
            }
            None => State {
                status: Some("Nothing to undo".to_owned()),
                ..state
            },
        },
        Effect::Save => save(table, state),
    }
}

/// Write the table back, reporting the outcome in the footer.
fn save(table: &mut Table, state: State) -> State {
    if !table.dirty {
        return State {
            status: Some("No changes to write".to_owned()),
            ..state
        };
    }

    let status = match write::save(table) {
        Ok(saved) => {
            table.dirty = false;
            // The file miolo just wrote is the new baseline; without this the
            // next write would mistake its own change for someone else's.
            table.origin.modified = saved.modified;
            let name = saved.path.file_name().map_or_else(
                || saved.path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            // Ragged input was squared up at load, so saying so is the only
            // honest way to report having rewritten it.
            if table.warnings.is_empty() {
                format!("Wrote {name} ({} rows)", table.len())
            } else {
                format!(
                    "Wrote {name} ({} rows, {} malformed rows normalised)",
                    table.len(),
                    table.warnings.len()
                )
            }
        }
        Err(error) => format!("Save failed: {error}"),
    };

    State {
        status: Some(status),
        ..state
    }
}

/// Copy the selected field's raw text, reporting the outcome in the footer.
fn yank(state: &State, table: &Table) -> State {
    let field = match state.mode {
        Mode::Pager => state.pager.field,
        _ => state.field,
    };
    let text = table.field(state.row, field);
    let name = table.headers.get(field).map_or("field", String::as_str);

    let status = match clipboard::copy(text) {
        Ok(true) => format!(
            "Yanked {name} (truncated to {}KB)",
            clipboard::MAX_BYTES / 1024
        ),
        Ok(false) => format!("Yanked {name} ({} bytes)", text.len()),
        Err(error) => format!("Yank failed: {error}"),
    };

    State {
        status: Some(status),
        ..state.clone()
    }
}
