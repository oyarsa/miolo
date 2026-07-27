//! Help overlay content.
//!
//! Kept separate from its rendering so the transition function can count lines
//! and clamp scrolling without reaching into the UI layer.

use crate::data::Table;

/// One line of the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelpLine {
    Section(&'static str),
    Binding {
        keys: &'static str,
        text: &'static str,
    },
    Blank,
    Warning {
        row: usize,
        text: String,
    },
    Note(String),
}

/// Bindings, grouped the way the design document lists them.
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("?", "This help"),
            ("q", "Quit (record) or step back"),
            ("Esc", "Back; cancel a prompt"),
            (":N", "Jump to row N (:$ for the last)"),
            ("/", "Search column names; field content in the pager"),
            ("n / N", "Next / previous match"),
            ("w", "Toggle wrap and truncate"),
            ("t", "Toggle record and table views"),
            ("y", "Yank the selected field"),
        ],
    ),
    (
        "Record",
        &[
            ("h l", "Previous / next row"),
            ("j k", "Previous / next field"),
            ("^d ^u", "Scroll half a page"),
            ("g G", "First / last field"),
            ("z", "Expand or collapse the selected field"),
            ("Enter", "Open the field in the pager"),
        ],
    ),
    (
        "Pager",
        &[
            ("j k", "Scroll one line"),
            ("^d ^u", "Scroll half a page"),
            ("h l", "Shift sideways (chops while shifted)"),
            ("g G", "Top / bottom"),
        ],
    ),
    (
        "Table",
        &[
            ("j k", "Previous / next row"),
            ("H L", "Scroll columns"),
            ("g G", "First / last row"),
            ("Enter", "Open the row in the record view"),
        ],
    ),
];

/// Warnings listed individually before collapsing into a count.
const MAX_LISTED_WARNINGS: usize = 50;

/// The whole overlay, bindings followed by any load warnings.
pub fn content(table: &Table) -> Vec<HelpLine> {
    let mut out = Vec::new();
    for (title, bindings) in SECTIONS {
        out.push(HelpLine::Section(title));
        for (keys, text) in *bindings {
            out.push(HelpLine::Binding { keys, text });
        }
        out.push(HelpLine::Blank);
    }

    if !table.warnings.is_empty() {
        out.push(HelpLine::Section("Load warnings"));
        for warning in table.warnings.iter().take(MAX_LISTED_WARNINGS) {
            out.push(HelpLine::Warning {
                row: warning.row,
                text: warning.kind.describe(),
            });
        }
        if table.warnings.len() > MAX_LISTED_WARNINGS {
            out.push(HelpLine::Note(format!(
                "\u{2026} and {} more",
                table.warnings.len() - MAX_LISTED_WARNINGS
            )));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_csv;

    #[test]
    fn lists_every_section() {
        let table = parse_csv(b"a\n1\n", "t").expect("parse failed");
        let sections: Vec<_> = content(&table)
            .into_iter()
            .filter_map(|l| match l {
                HelpLine::Section(name) => Some(name),
                _ => None,
            })
            .collect();
        assert_eq!(sections, ["Global", "Record", "Pager", "Table"]);
    }

    #[test]
    fn appends_load_warnings() {
        let table = parse_csv(b"a,b\n1\n", "t").expect("parse failed");
        let lines = content(&table);
        assert!(lines.contains(&HelpLine::Section("Load warnings")));
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, HelpLine::Warning { row: 1, .. }))
        );
    }

    #[test]
    fn clean_files_have_no_warning_section() {
        let table = parse_csv(b"a,b\n1,2\n", "t").expect("parse failed");
        assert!(!content(&table).contains(&HelpLine::Section("Load warnings")));
    }

    #[test]
    fn caps_the_warning_list() {
        let mut csv = String::from("a,b\n");
        for _ in 0..(MAX_LISTED_WARNINGS + 10) {
            csv.push_str("1\n");
        }
        let table = parse_csv(csv.as_bytes(), "t").expect("parse failed");
        let lines = content(&table);
        let listed = lines
            .iter()
            .filter(|l| matches!(l, HelpLine::Warning { .. }))
            .count();
        assert_eq!(listed, MAX_LISTED_WARNINGS);
        assert!(lines.iter().any(|l| matches!(l, HelpLine::Note(_))));
    }
}
