//! Loading input into memory.
//!
//! Parsing is a pure function from bytes to a [`Table`]; only [`load`] touches
//! the filesystem. Every format produces the same table, so nothing downstream
//! of here knows what the input was.

mod delimited;
mod json;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result};

use crate::decompress::decompress;
use crate::source::{Format, FormatArg, detect_compression, resolve_format};

/// Why a record needed reconciling, or could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// Row had fewer fields than the header; padded with empties.
    Short { got: usize, want: usize },
    /// Row had more fields than the header; surplus columns were synthesised.
    Long { got: usize, want: usize },
    /// Row contained bytes that are not valid UTF-8; replaced lossily.
    InvalidUtf8,
    /// A JSON record that was not an object; rendered as an empty row.
    NotAnObject { found: String },
    /// A JSONL line that would not parse; rendered as an empty row.
    MalformedJson(String),
}

impl WarningKind {
    /// Human-readable explanation, for the help overlay.
    pub fn describe(&self) -> String {
        match self {
            Self::Short { got, want } => format!("{got} fields, expected {want} (padded)"),
            Self::Long { got, want } => format!("{got} fields, expected {want} (surplus kept)"),
            Self::InvalidUtf8 => "invalid UTF-8 (replaced)".to_owned(),
            Self::NotAnObject { found } => format!("expected an object, found {found} (empty row)"),
            Self::MalformedJson(error) => format!("invalid JSON: {error} (empty row)"),
        }
    }
}

/// A problem found while loading one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadWarning {
    /// One-based data row number, matching what the UI displays.
    pub row: usize,
    pub kind: WarningKind,
}

/// A whole input held in memory.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub warnings: Vec<LoadWarning>,
    /// Display name for the status bar.
    pub name: String,
}

impl Table {
    /// Number of data rows, excluding the header.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of columns, after reconciling every row against the header.
    pub fn width(&self) -> usize {
        self.headers.len()
    }

    /// Raw text of one field, or `""` when either index is out of range.
    pub fn field(&self, row: usize, col: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(col))
            .map_or("", String::as_str)
    }
}

/// Parse already-uncompressed bytes in a known format.
pub fn parse(data: &[u8], format: Format, name: &str) -> Result<Table> {
    match format {
        Format::Delimited(delimiter) => delimited::parse(data, delimiter, name)
            .with_context(|| format!("could not read {name} as separated values")),
        Format::Json => json::parse_array(data, name),
        Format::Jsonl => Ok(json::parse_lines(data, name)),
    }
}

/// Parse comma-separated bytes. A convenience for tests elsewhere in the
/// crate, which care about the resulting table rather than the format.
#[cfg(test)]
pub fn parse_csv(data: &[u8], name: &str) -> Result<Table> {
    parse(data, Format::Delimited(b','), name)
}

/// Read an input, uncompress it if needed, and parse it.
pub fn load(
    path: Option<&Path>,
    requested: Option<FormatArg>,
    delimiter: Option<u8>,
) -> Result<Table> {
    let (data, name) = read(path)?;

    // Compression is identified from the bytes, so a misleading name still
    // opens and piped input needs no flag. The format is never guessed.
    let compression = detect_compression(&data);
    let data =
        decompress(data, compression).with_context(|| format!("could not decompress {name}"))?;

    let format = resolve_format(path, requested, delimiter);
    parse(&data, format, &name)
}

/// Slurp a file, or standard input when `path` is `None` or `-`.
fn read(path: Option<&Path>) -> Result<(Vec<u8>, String)> {
    let mut data = Vec::new();
    let name = match path {
        Some(p) if p != Path::new("-") => {
            File::open(p)
                .with_context(|| format!("could not open {}", p.display()))?
                .read_to_end(&mut data)
                .with_context(|| format!("could not read {}", p.display()))?;
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        }
        _ => {
            io::stdin()
                .read_to_end(&mut data)
                .context("could not read standard input")?;
            "<stdin>".to_owned()
        }
    };
    Ok((data, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_accessors_handle_out_of_range() {
        let table = parse(b"a,b\n1,2\n", Format::Delimited(b','), "t").expect("parse failed");
        assert_eq!(table.len(), 1);
        assert_eq!(table.width(), 2);
        assert_eq!(table.field(9, 0), "");
        assert_eq!(table.field(0, 9), "");
        assert!(!table.is_empty());
    }

    #[test]
    fn parse_dispatches_on_format() {
        let csv = parse(b"a,b\n1,2\n", Format::Delimited(b','), "t").expect("csv");
        assert_eq!(csv.headers, ["a", "b"]);

        let json = parse(br#"[{"a":1}]"#, Format::Json, "t").expect("json");
        assert_eq!(json.headers, ["a"]);

        let jsonl = parse(br#"{"a":1}"#, Format::Jsonl, "t").expect("jsonl");
        assert_eq!(jsonl.headers, ["a"]);
    }

    #[test]
    fn warning_descriptions_name_the_problem() {
        assert!(
            WarningKind::Short { got: 1, want: 3 }
                .describe()
                .contains("expected 3")
        );
        assert!(
            WarningKind::NotAnObject {
                found: "a number".to_owned()
            }
            .describe()
            .contains("a number")
        );
        assert!(
            WarningKind::MalformedJson("trailing comma".to_owned())
                .describe()
                .contains("trailing comma")
        );
    }
}
