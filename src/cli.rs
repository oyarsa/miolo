//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;

use crate::source::FormatArg;

/// GNU-style long version string with copyright and license.
///
/// Note: Update the date literal below when cutting a new release.
fn long_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        " (2026-07-27)\n\n", // Update date when releasing
        "Copyright (C) 2026 Italo Silva\n",
        "License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>\n",
        "This is free software: you are free to change and redistribute it.\n",
        "There is NO WARRANTY, to the extent permitted by law."
    )
}

/// Terminal viewer for CSV, TSV, JSON and JSONL with long, multi-line text columns.
#[derive(Parser, Debug)]
#[command(name = "miolo")]
#[command(
    about = "Terminal viewer for CSV, TSV, JSON and JSONL with long, multi-line text columns"
)]
#[command(version, long_version = long_version())]
pub struct Cli {
    /// File to view. Use "-", or omit it with a pipe, to read stdin
    pub file: Option<PathBuf>,

    /// Input format [default: inferred from the file extension]
    #[arg(short, long, value_name = "FORMAT", value_enum)]
    pub format: Option<FormatArg>,

    /// Field delimiter for separated-value formats [default: ,]
    #[arg(short, long, value_name = "CHAR")]
    pub delimiter: Option<char>,

    /// Max field height, as a percentage of the record body
    #[arg(
        short,
        long,
        value_name = "PCT",
        default_value_t = 40,
        value_parser = clap::value_parser!(u8).range(1..=100),
    )]
    pub max_height: u8,

    /// Start in truncate mode instead of wrapping
    #[arg(long)]
    pub no_wrap: bool,

    /// Disable colour (`NO_COLOR` is also honoured)
    #[arg(long, help = "Disable colour (NO_COLOR is also honoured)")]
    pub no_color: bool,
}
