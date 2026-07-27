//! miolo — a terminal viewer for CSV files with long, multi-line text columns.

mod cli;
mod clipboard;
mod data;
mod help;
mod keys;
mod layout;
mod markdown;
mod search;
mod state;
mod ui;

use std::io::{self, IsTerminal, Stdout};

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
use crate::data::Table;
use crate::layout::field_cap;
use crate::state::{Action, Mode, State, Viewport, update};
use crate::ui::{Theme, record};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.file.is_none() && io::stdin().is_terminal() {
        // Nothing piped and no path given: there is nothing to show.
        Cli::parse_from(["miolo", "--help"]);
        return Ok(());
    }

    let delimiter = delimiter_byte(cli.delimiter)?;
    let table = data::load(cli.file.as_deref(), delimiter).context("failed to read input")?;
    let theme = Theme::new(!cli.no_color && std::env::var_os("NO_COLOR").is_none());

    let mut terminal = setup().context("failed to set up the terminal")?;
    let outcome = run(&mut terminal, &table, &cli, theme);
    restore().context("failed to restore the terminal")?;
    outcome
}

/// A delimiter has to be a single byte for the CSV reader.
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

fn run(terminal: &mut Terminal<Backend>, table: &Table, cli: &Cli, theme: Theme) -> Result<()> {
    let mut state = State::new(!cli.no_wrap);
    // Sampling column widths is expensive on a large file, so do it once.
    let ctx = ui::Context::new(theme, table);

    loop {
        let view = viewport(terminal.size()?, cli.max_height);
        terminal.draw(|frame| ui::render(frame, &state, table, view, &ctx))?;

        let action = match event::read()? {
            Event::Key(key) => keys::action_for(key, state.mode, state.prompt.is_some()),
            Event::Mouse(mouse) => keys::action_for_mouse(mouse.kind, mouse.row),
            _ => None,
        };

        if let Some(action) = action {
            // Yank is the one action with a side effect, so it is performed
            // here rather than inside the pure transition function.
            state = if action == Action::Yank && state.prompt.is_none() {
                yank(&state, table)
            } else {
                update(&state, table, view, action)
            };
        }

        if state.quit {
            return Ok(());
        }
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
