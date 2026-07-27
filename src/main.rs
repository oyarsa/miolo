//! miolo — a terminal viewer for CSV files with long, multi-line text columns.

mod cli;

use clap::Parser;

use crate::cli::Cli;

fn main() {
    let cli = Cli::parse();
    println!("{cli:?}");
}
