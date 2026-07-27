//! Loading CSV data into memory.
//!
//! Parsing is a pure function from bytes to a [`Table`]; only [`load`] touches
//! the filesystem. Malformed input never fails the load — ragged rows are
//! reconciled against the header and recorded as [`LoadWarning`]s.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Why a row needed reconciling against the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// Row had fewer fields than the header; padded with empties.
    Short { got: usize, want: usize },
    /// Row had more fields than the header; surplus columns were synthesised.
    Long { got: usize, want: usize },
    /// Row contained bytes that are not valid UTF-8; replaced lossily.
    InvalidUtf8,
}

impl WarningKind {
    /// Human-readable explanation, for the help overlay.
    pub fn describe(self) -> String {
        match self {
            Self::Short { got, want } => format!("{got} fields, expected {want} (padded)"),
            Self::Long { got, want } => format!("{got} fields, expected {want} (surplus kept)"),
            Self::InvalidUtf8 => "invalid UTF-8 (replaced)".to_owned(),
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

/// A whole CSV file held in memory.
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

/// Parse CSV bytes into a [`Table`].
///
/// The first record is always the header. Rows are reconciled to a common
/// width: short rows are padded with empty fields, and columns beyond the
/// header are named `+1`, `+2`, and so on.
pub fn parse(data: &[u8], delimiter: u8, name: &str) -> Result<Table, csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(true)
        .from_reader(data);

    let mut warnings = Vec::new();

    let headers: Vec<String> = reader
        .byte_headers()?
        .iter()
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut record = csv::ByteRecord::new();
    while reader.read_byte_record(&mut record)? {
        let number = rows.len() + 1;
        let mut lossy = false;
        let fields: Vec<String> = record
            .iter()
            .map(|f| {
                lossy |= std::str::from_utf8(f).is_err();
                String::from_utf8_lossy(f).into_owned()
            })
            .collect();

        if lossy {
            warnings.push(LoadWarning {
                row: number,
                kind: WarningKind::InvalidUtf8,
            });
        }
        rows.push(fields);
    }

    let widest = rows.iter().map(Vec::len).max().unwrap_or(0);
    let width = headers.len().max(widest);

    // Record ragged rows before padding, so the counts reported are the ones
    // actually present in the file.
    for (index, row) in rows.iter().enumerate() {
        let kind = match row.len() {
            got if got < headers.len() => Some(WarningKind::Short {
                got,
                want: headers.len(),
            }),
            got if got > headers.len() => Some(WarningKind::Long {
                got,
                want: headers.len(),
            }),
            _ => None,
        };
        if let Some(kind) = kind {
            warnings.push(LoadWarning {
                row: index + 1,
                kind,
            });
        }
    }

    let mut headers = headers;
    for offset in 0..width.saturating_sub(headers.len()) {
        headers.push(format!("+{}", offset + 1));
    }

    for row in &mut rows {
        row.resize(width, String::new());
    }

    warnings.sort_by_key(|w| w.row);

    Ok(Table {
        headers,
        rows,
        warnings,
        name: name.to_owned(),
    })
}

/// Read a CSV file, or standard input when `path` is `None` or `-`.
pub fn load(path: Option<&Path>, delimiter: u8) -> io::Result<Table> {
    let mut data = Vec::new();
    let name = match path {
        Some(p) if p != Path::new("-") => {
            File::open(p)?.read_to_end(&mut data)?;
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        }
        _ => {
            io::stdin().read_to_end(&mut data)?;
            "<stdin>".to_owned()
        }
    };

    parse(&data, delimiter, &name).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(csv: &str) -> Table {
        parse(csv.as_bytes(), b',', "test").expect("parse failed")
    }

    #[test]
    fn reads_headers_and_rows() {
        let t = table("a,b\n1,2\n3,4\n");
        assert_eq!(t.headers, ["a", "b"]);
        assert_eq!(t.rows, [["1", "2"], ["3", "4"]]);
        assert_eq!(t.len(), 2);
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn keeps_embedded_newlines_and_quotes() {
        let t = table("a,b\n\"line one\nline two\",\"say \"\"hi\"\"\"\n");
        assert_eq!(t.field(0, 0), "line one\nline two");
        assert_eq!(t.field(0, 1), "say \"hi\"");
    }

    #[test]
    fn pads_short_rows_and_warns() {
        let t = table("a,b,c\n1\n");
        assert_eq!(t.rows, [["1", "", ""]]);
        assert_eq!(
            t.warnings,
            [LoadWarning {
                row: 1,
                kind: WarningKind::Short { got: 1, want: 3 }
            }]
        );
    }

    #[test]
    fn names_surplus_columns_and_warns() {
        let t = table("a,b\n1,2,3,4\n");
        assert_eq!(t.headers, ["a", "b", "+1", "+2"]);
        assert_eq!(t.rows, [["1", "2", "3", "4"]]);
        assert_eq!(
            t.warnings,
            [LoadWarning {
                row: 1,
                kind: WarningKind::Long { got: 4, want: 2 }
            }]
        );
    }

    #[test]
    fn pads_every_row_to_the_widest() {
        let t = table("a,b\n1,2,3\n4,5\n");
        assert_eq!(t.width(), 3);
        assert_eq!(t.rows[1], ["4", "5", ""]);
    }

    #[test]
    fn replaces_invalid_utf8() {
        let mut data = b"a,b\nok,".to_vec();
        data.push(0xff);
        data.push(b'\n');
        let t = parse(&data, b',', "test").expect("parse failed");
        assert_eq!(t.field(0, 1), "\u{fffd}");
        assert_eq!(t.warnings[0].kind, WarningKind::InvalidUtf8);
    }

    #[test]
    fn honours_the_delimiter() {
        let t = parse(b"a\tb\n1\t2\n", b'\t', "test").expect("parse failed");
        assert_eq!(t.headers, ["a", "b"]);
        assert_eq!(t.rows, [["1", "2"]]);
    }

    #[test]
    fn header_only_file_has_no_rows() {
        let t = table("a,b\n");
        assert!(t.is_empty());
        assert_eq!(t.width(), 2);
    }

    #[test]
    fn field_out_of_range_is_empty() {
        let t = table("a,b\n1,2\n");
        assert_eq!(t.field(9, 0), "");
        assert_eq!(t.field(0, 9), "");
    }

    #[test]
    fn warnings_are_ordered_by_row() {
        let t = table("a,b\n1\n1,2\n1,2,3\n");
        let rows: Vec<usize> = t.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [1, 3]);
    }
}
