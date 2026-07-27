//! Separated-value parsing.
//!
//! Unchanged from the original CSV path: malformed input never fails the load,
//! it is reconciled against the header and recorded as a warning.

use super::{LoadWarning, Table, WarningKind};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{LoadWarning, WarningKind};

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
