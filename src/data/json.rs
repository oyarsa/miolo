//! JSON and JSONL parsing.
//!
//! Both formats reduce to a sequence of objects, so they share everything past
//! their front ends. Structural problems are fatal; a bad individual record
//! becomes a row of empty fields with a warning, matching how ragged rows
//! already behave in delimited input.

use std::collections::HashSet;

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::{LoadWarning, Table, WarningKind};

/// One input record, or the reason it could not be used.
enum Record {
    Object(Map<String, Value>),
    Bad(WarningKind),
}

/// Parse a whole document containing an array of objects.
pub fn parse_array(data: &[u8], name: &str) -> Result<Table> {
    let value: Value =
        serde_json::from_slice(data).map_err(|error| anyhow::anyhow!("invalid JSON: {error}"))?;

    let Value::Array(items) = value else {
        bail!(
            "expected a top-level JSON array of objects, found {}",
            describe_value(&value)
        );
    };

    let records = items
        .into_iter()
        .map(|item| match item {
            Value::Object(map) => Record::Object(map),
            other => Record::Bad(WarningKind::NotAnObject {
                found: describe_value(&other).to_owned(),
            }),
        })
        .collect();

    Ok(assemble(records, name))
}

/// Parse one object per line, skipping blank lines.
///
/// Unlike a whole-document parse, a syntax error here costs only its own line:
/// each line stands alone, so there is nothing for it to invalidate.
pub fn parse_lines(data: &[u8], name: &str) -> Table {
    let text = String::from_utf8_lossy(data);
    let records = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(map)) => Record::Object(map),
            Ok(other) => Record::Bad(WarningKind::NotAnObject {
                found: describe_value(&other).to_owned(),
            }),
            Err(error) => Record::Bad(WarningKind::MalformedJson(error.to_string())),
        })
        .collect();

    assemble(records, name)
}

/// Turn records into a table, taking columns from the union of their keys.
fn assemble(records: Vec<Record>, name: &str) -> Table {
    let headers = union_of_keys(&records);

    let mut rows = Vec::with_capacity(records.len());
    let mut warnings = Vec::new();

    for (index, record) in records.into_iter().enumerate() {
        match record {
            Record::Object(map) => rows.push(
                headers
                    .iter()
                    .map(|key| map.get(key).map_or_else(String::new, render))
                    .collect(),
            ),
            Record::Bad(kind) => {
                warnings.push(LoadWarning {
                    row: index + 1,
                    kind,
                });
                // An empty row rather than no row, so row numbers keep lining
                // up with positions in the file.
                rows.push(vec![String::new(); headers.len()]);
            }
        }
    }

    Table {
        headers,
        rows,
        warnings,
        name: name.to_owned(),
    }
}

/// Every key across every record, in the order first encountered.
fn union_of_keys(records: &[Record]) -> Vec<String> {
    let mut headers = Vec::new();
    let mut seen = HashSet::new();
    for record in records {
        if let Record::Object(map) = record {
            for key in map.keys() {
                if seen.insert(key.clone()) {
                    headers.push(key.clone());
                }
            }
        }
    }
    headers
}

/// Render a value as the text of a field.
///
/// Scalars render bare; anything structured is pretty-printed and then behaves
/// like any other tall field. Numbers keep their source token exactly, so
/// `1.0` does not become `1`.
fn render(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        structured => {
            serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string())
        }
    }
}

/// A human-readable name for a value's type, for error messages.
fn describe_value(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
