//! Tests for the JSON and JSONL front ends.

use super::*;

fn array(text: &str) -> Table {
    parse_array(text.as_bytes(), "test.json").expect("parse failed")
}

fn lines(text: &str) -> Table {
    parse_lines(text.as_bytes(), "test.jsonl")
}

#[test]
fn reads_an_array_of_objects() {
    let table = array(r#"[{"id":1,"name":"ada"},{"id":2,"name":"bob"}]"#);
    assert_eq!(table.headers, ["id", "name"]);
    assert_eq!(table.rows, [["1", "ada"], ["2", "bob"]]);
    assert!(table.warnings.is_empty());
}

#[test]
fn reads_one_object_per_line() {
    let table = lines("{\"id\":1}\n{\"id\":2}\n");
    assert_eq!(table.headers, ["id"]);
    assert_eq!(table.rows, [["1"], ["2"]]);
}

#[test]
fn blank_lines_are_not_records() {
    let table = lines("{\"id\":1}\n\n   \n{\"id\":2}\n");
    assert_eq!(table.len(), 2);
    assert!(table.warnings.is_empty());
}

#[test]
fn columns_are_the_union_in_first_seen_order() {
    let table = array(r#"[{"b":1},{"a":2},{"c":3,"a":4}]"#);
    assert_eq!(table.headers, ["b", "a", "c"]);
}

#[test]
fn absent_keys_render_empty() {
    let table = array(r#"[{"a":1,"b":2},{"a":3}]"#);
    assert_eq!(table.rows[1], ["3", ""]);
}

#[test]
fn null_renders_empty_like_an_absent_key() {
    let table = array(r#"[{"a":null,"b":1}]"#);
    assert_eq!(table.field(0, 0), "", "null collapses with empty");
}

#[test]
fn scalars_render_bare() {
    let table = array(r#"[{"s":"text","n":42,"t":true,"f":false}]"#);
    assert_eq!(table.rows[0], ["text", "42", "true", "false"]);
}

#[test]
fn strings_are_not_quoted() {
    let table = array(r#"[{"a":"has \"quotes\" inside"}]"#);
    assert_eq!(table.field(0, 0), r#"has "quotes" inside"#);
}

#[test]
fn numbers_keep_their_digits() {
    let table = array(
        r#"[{"a":1.0,"b":1.50,"c":10000000000000000000000,"d":0.1234567890123456789,"e":-0.0}]"#,
    );
    assert_eq!(table.field(0, 0), "1.0", "a trailing zero survives");
    assert_eq!(table.field(0, 1), "1.50");
    assert_eq!(
        table.field(0, 2),
        "10000000000000000000000",
        "a big integer keeps every digit"
    );
    assert_eq!(
        table.field(0, 3),
        "0.1234567890123456789",
        "more precision than an f64 could hold"
    );
    assert_eq!(table.field(0, 4), "-0.0", "signed zero survives");
}

#[test]
fn exponent_notation_is_normalised() {
    // The one thing not preserved verbatim: the exponent marker is lowercased
    // and given an explicit sign. The value is unchanged.
    let table = array(r#"[{"a":1e3,"b":1E3,"c":1e-3}]"#);
    assert_eq!(table.field(0, 0), "1e+3");
    assert_eq!(table.field(0, 1), "1e+3");
    assert_eq!(table.field(0, 2), "1e-3");
}

#[test]
fn nested_values_are_pretty_printed() {
    let table = array(r#"[{"items":[{"sku":"AB-12","qty":2}]}]"#);
    let cell = table.field(0, 0);
    assert!(cell.starts_with('['), "keeps its JSON shape");
    assert!(cell.contains('\n'), "spans multiple lines");
    assert!(
        cell.contains("\"sku\": \"AB-12\""),
        "pretty-printed spacing"
    );
}

#[test]
fn embedded_newlines_in_strings_survive() {
    let table = array(r#"[{"notes":"first\nsecond"}]"#);
    assert_eq!(table.field(0, 0), "first\nsecond");
}

#[test]
fn an_empty_array_has_no_rows_or_columns() {
    let table = array("[]");
    assert!(table.is_empty());
    assert_eq!(table.width(), 0);
}

// -- errors and recovery -------------------------------------------------

#[test]
fn a_top_level_object_is_fatal() {
    let error = parse_array(br#"{"a":1}"#, "t").expect_err("not an array");
    assert!(
        error.to_string().contains("array"),
        "says what was expected"
    );
    assert!(
        error.to_string().contains("an object"),
        "says what it found"
    );
}

#[test]
fn a_top_level_scalar_is_fatal() {
    assert!(parse_array(b"42", "t").is_err());
    assert!(parse_array(br#""text""#, "t").is_err());
}

#[test]
fn a_whole_document_syntax_error_is_fatal() {
    let error = parse_array(br#"[{"a":1},]"#, "t").expect_err("trailing comma");
    assert!(error.to_string().contains("invalid JSON"));
}

#[test]
fn an_array_element_that_is_not_an_object_becomes_an_empty_row() {
    let table = array(r#"[{"a":1},42,{"a":3}]"#);
    assert_eq!(table.len(), 3, "the bad element still occupies a row");
    assert_eq!(table.rows[1], [""], "rendered empty");
    assert_eq!(table.rows[2], ["3"], "later rows still align");

    assert_eq!(table.warnings.len(), 1);
    assert_eq!(table.warnings[0].row, 2, "warning names the right row");
    assert!(matches!(
        table.warnings[0].kind,
        WarningKind::NotAnObject { .. }
    ));
}

#[test]
fn a_bad_jsonl_line_costs_only_itself() {
    let table = lines("{\"a\":1}\nnot json at all\n{\"a\":3}\n");
    assert_eq!(table.len(), 3);
    assert_eq!(table.rows[0], ["1"]);
    assert_eq!(table.rows[1], [""]);
    assert_eq!(table.rows[2], ["3"], "parsing continues past the bad line");

    assert_eq!(table.warnings.len(), 1);
    assert_eq!(table.warnings[0].row, 2);
    assert!(matches!(
        table.warnings[0].kind,
        WarningKind::MalformedJson(_)
    ));
}

#[test]
fn a_jsonl_line_that_is_not_an_object_becomes_an_empty_row() {
    let table = lines("{\"a\":1}\n[1,2,3]\n");
    assert_eq!(table.len(), 2);
    assert!(matches!(
        table.warnings[0].kind,
        WarningKind::NotAnObject { .. }
    ));
}

#[test]
fn every_line_being_bad_still_produces_a_table() {
    let table = lines("nope\nalso nope\n");
    assert_eq!(table.len(), 2);
    assert_eq!(table.width(), 0, "no keys were ever seen");
    assert_eq!(table.warnings.len(), 2);
}

#[test]
fn invalid_utf8_does_not_panic() {
    let mut data = br#"{"a":1}"#.to_vec();
    data.push(0xff);
    data.push(b'\n');
    let table = parse_lines(&data, "t");
    assert_eq!(table.len(), 1);
}
