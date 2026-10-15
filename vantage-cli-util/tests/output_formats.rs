//! Output formatters: `cbor-diag` (lossless), `json` (lossy), `ndjson`, and
//! the `OutputFormat` dispatch.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_cli_util::OutputFormat;
use vantage_cli_util::output::{self, cbor_diag, json, ndjson};
use vantage_types::Record;

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

fn two_users() -> IndexMap<String, Record<CborValue>> {
    let mut records = IndexMap::new();
    records.insert("u1".to_string(), record(&[("name", text("alice"))]));
    records.insert("u2".to_string(), record(&[("name", text("bob"))]));
    records
}

// ─── OutputFormat ──────────────────────────────────────────────────────────

#[test]
fn format_names_parse() {
    assert_eq!(OutputFormat::parse("table"), Some(OutputFormat::Table));
    assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
    assert_eq!(OutputFormat::parse("ndjson"), Some(OutputFormat::Ndjson));
    assert_eq!(
        OutputFormat::parse("cbor-diag"),
        Some(OutputFormat::CborDiag)
    );
}

#[test]
fn unknown_format_name_parses_to_none() {
    assert_eq!(OutputFormat::parse("yaml"), None);
    assert_eq!(OutputFormat::parse(""), None);
}

#[test]
fn default_format_is_table() {
    assert_eq!(OutputFormat::default(), OutputFormat::Table);
}

#[test]
fn table_format_renders_nothing_through_render_helpers() {
    let records = two_users();
    let r = record(&[("name", text("alice"))]);
    assert_eq!(output::render_list(OutputFormat::Table, &records), "");
    assert_eq!(output::render_record(OutputFormat::Table, "u1", &r), "");
    assert_eq!(output::render_scalar(OutputFormat::Table, "n", &int(1)), "");
}

#[test]
fn render_helpers_dispatch_to_the_chosen_writer() {
    let records = two_users();
    let r = record(&[("name", text("alice"))]);
    let v = int(5);

    assert_eq!(
        output::render_list(OutputFormat::Json, &records),
        json::write_list(&records)
    );
    assert_eq!(
        output::render_list(OutputFormat::Ndjson, &records),
        ndjson::write_list(&records)
    );
    assert_eq!(
        output::render_list(OutputFormat::CborDiag, &records),
        cbor_diag::write_list(&records)
    );

    assert_eq!(
        output::render_record(OutputFormat::Json, "u1", &r),
        json::write_record("u1", &r)
    );
    assert_eq!(
        output::render_record(OutputFormat::Ndjson, "u1", &r),
        ndjson::write_record("u1", &r)
    );
    assert_eq!(
        output::render_record(OutputFormat::CborDiag, "u1", &r),
        cbor_diag::write_record("u1", &r)
    );

    assert_eq!(
        output::render_scalar(OutputFormat::Json, "n", &v),
        json::write_scalar("n", &v)
    );
    assert_eq!(
        output::render_scalar(OutputFormat::Ndjson, "n", &v),
        ndjson::write_scalar("n", &v)
    );
    assert_eq!(
        output::render_scalar(OutputFormat::CborDiag, "n", &v),
        cbor_diag::write_scalar("n", &v)
    );
}

// ─── cbor-diag ─────────────────────────────────────────────────────────────

#[test]
fn cbor_diag_scalars() {
    assert_eq!(cbor_diag::write_value(&int(42)), "42");
    assert_eq!(cbor_diag::write_value(&int(-7)), "-7");
    assert_eq!(cbor_diag::write_value(&CborValue::Bool(true)), "true");
    assert_eq!(cbor_diag::write_value(&CborValue::Bool(false)), "false");
    assert_eq!(cbor_diag::write_value(&CborValue::Null), "null");
}

#[test]
fn cbor_diag_floats_always_carry_a_decimal_point() {
    assert_eq!(cbor_diag::write_value(&CborValue::Float(2.5)), "2.5");
    assert_eq!(cbor_diag::write_value(&CborValue::Float(42.0)), "42.0");
    assert_eq!(
        cbor_diag::write_value(&CborValue::Float(1e10)),
        "10000000000.0"
    );
}

#[test]
fn cbor_diag_float_specials_have_names() {
    assert_eq!(cbor_diag::write_value(&CborValue::Float(f64::NAN)), "NaN");
    assert_eq!(
        cbor_diag::write_value(&CborValue::Float(f64::INFINITY)),
        "Infinity"
    );
    assert_eq!(
        cbor_diag::write_value(&CborValue::Float(f64::NEG_INFINITY)),
        "-Infinity"
    );
}

#[test]
fn cbor_diag_text_is_quoted_and_escaped() {
    assert_eq!(cbor_diag::write_value(&text("hi")), "\"hi\"");
    assert_eq!(cbor_diag::write_value(&text("a\"b")), "\"a\\\"b\"");
    assert_eq!(cbor_diag::write_value(&text("a\\b")), "\"a\\\\b\"");
    assert_eq!(cbor_diag::write_value(&text("l\nm")), "\"l\\nm\"");
    assert_eq!(cbor_diag::write_value(&text("l\rm")), "\"l\\rm\"");
    assert_eq!(cbor_diag::write_value(&text("l\tm")), "\"l\\tm\"");
}

#[test]
fn cbor_diag_other_control_characters_use_unicode_escapes() {
    assert_eq!(cbor_diag::write_value(&text("\u{0001}")), "\"\\u{0001}\"");
}

#[test]
fn cbor_diag_bytes_are_hex_in_h_quotes() {
    assert_eq!(
        cbor_diag::write_value(&CborValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef])),
        "h'deadbeef'"
    );
    assert_eq!(cbor_diag::write_value(&CborValue::Bytes(vec![])), "h''");
}

#[test]
fn cbor_diag_arrays_and_maps() {
    let arr = CborValue::Array(vec![int(1), text("two"), CborValue::Bool(true)]);
    assert_eq!(cbor_diag::write_value(&arr), "[1, \"two\", true]");

    let map = CborValue::Map(vec![(text("a"), int(1)), (text("b"), CborValue::Null)]);
    assert_eq!(cbor_diag::write_value(&map), "{\"a\": 1, \"b\": null}");

    assert_eq!(cbor_diag::write_value(&CborValue::Array(vec![])), "[]");
    assert_eq!(cbor_diag::write_value(&CborValue::Map(vec![])), "{}");
}

#[test]
fn cbor_diag_map_keys_render_as_diagnostic_values() {
    let map = CborValue::Map(vec![(int(1), text("one"))]);
    assert_eq!(cbor_diag::write_value(&map), "{1: \"one\"}");
}

#[test]
fn cbor_diag_tagged_values_render_as_tag_call() {
    let t = CborValue::Tag(0, Box::new(text("2024-01-01")));
    assert_eq!(cbor_diag::write_value(&t), "0(\"2024-01-01\")");
}

#[test]
fn cbor_diag_list_is_a_map_from_id_to_record() {
    assert_eq!(
        cbor_diag::write_list(&two_users()),
        "{\"u1\": {\"name\": \"alice\"}, \"u2\": {\"name\": \"bob\"}}\n"
    );
}

#[test]
fn cbor_diag_empty_list_is_an_empty_map() {
    assert_eq!(cbor_diag::write_list(&IndexMap::new()), "{}\n");
}

#[test]
fn cbor_diag_record_is_id_then_map() {
    let r = record(&[("name", text("alice")), ("age", int(30))]);
    assert_eq!(
        cbor_diag::write_record("u1", &r),
        "\"u1\": {\"name\": \"alice\", \"age\": 30}\n"
    );
}

#[test]
fn cbor_diag_scalar_is_label_then_value() {
    assert_eq!(
        cbor_diag::write_scalar("sum(price)", &int(42)),
        "\"sum(price)\": 42\n"
    );
}

// ─── json ──────────────────────────────────────────────────────────────────

#[test]
fn json_scalars() {
    assert_eq!(json::write_value(&int(42)), "42");
    assert_eq!(json::write_value(&int(-7)), "-7");
    assert_eq!(json::write_value(&CborValue::Float(2.5)), "2.5");
    assert_eq!(json::write_value(&CborValue::Bool(true)), "true");
    assert_eq!(json::write_value(&CborValue::Null), "null");
    assert_eq!(json::write_value(&text("hi")), "\"hi\"");
}

#[test]
fn json_text_is_escaped() {
    assert_eq!(json::write_value(&text("a\"b\nc")), "\"a\\\"b\\nc\"");
}

#[test]
fn json_integers_beyond_i64_become_decimal_strings() {
    let big = CborValue::Integer(u64::MAX.into());
    assert_eq!(json::write_value(&big), "\"18446744073709551615\"");
    let small = CborValue::Integer(i64::MIN.into());
    assert_eq!(json::write_value(&small), "-9223372036854775808");
}

#[test]
fn json_float_specials_become_null() {
    assert_eq!(json::write_value(&CborValue::Float(f64::NAN)), "null");
    assert_eq!(json::write_value(&CborValue::Float(f64::INFINITY)), "null");
    assert_eq!(
        json::write_value(&CborValue::Float(f64::NEG_INFINITY)),
        "null"
    );
}

#[test]
fn json_bytes_are_0x_hex_strings() {
    assert_eq!(
        json::write_value(&CborValue::Bytes(vec![0xab, 0xcd])),
        "\"0xabcd\""
    );
}

#[test]
fn json_tagged_values_drop_the_tag() {
    let t = CborValue::Tag(0, Box::new(text("2024-01-01")));
    assert_eq!(json::write_value(&t), "\"2024-01-01\"");
}

#[test]
fn json_arrays_and_maps_are_compact() {
    let arr = CborValue::Array(vec![int(1), text("two"), CborValue::Null]);
    assert_eq!(json::write_value(&arr), "[1,\"two\",null]");

    let map = CborValue::Map(vec![(text("a"), int(1)), (text("b"), CborValue::Null)]);
    assert_eq!(json::write_value(&map), "{\"a\":1,\"b\":null}");
}

#[test]
fn json_non_text_map_keys_become_strings() {
    let map = CborValue::Map(vec![(int(1), text("one"))]);
    assert_eq!(json::write_value(&map), "{\"1\":\"one\"}");
}

#[test]
fn json_record_map_is_a_bare_object() {
    let r = record(&[("name", text("alice")), ("age", int(30))]);
    assert_eq!(
        json::write_record_map(&r),
        "{\"name\":\"alice\",\"age\":30}"
    );
}

#[test]
fn json_record_is_keyed_by_id() {
    let r = record(&[("name", text("alice")), ("age", int(30))]);
    assert_eq!(
        json::write_record("u1", &r),
        "{\"u1\":{\"name\":\"alice\",\"age\":30}}\n"
    );
}

#[test]
fn json_list_is_one_object_keyed_by_id() {
    assert_eq!(
        json::write_list(&two_users()),
        "{\"u1\":{\"name\":\"alice\"},\"u2\":{\"name\":\"bob\"}}\n"
    );
}

#[test]
fn json_scalar_is_a_one_key_object() {
    assert_eq!(json::write_scalar("count", &int(5)), "{\"count\":5}\n");
}

// ─── ndjson ────────────────────────────────────────────────────────────────

#[test]
fn ndjson_list_has_one_line_per_record_with_an_id_handle() {
    let s = ndjson::write_list(&two_users());
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "{\"_id\":\"u1\",\"name\":\"alice\"}");
    assert_eq!(lines[1], "{\"_id\":\"u2\",\"name\":\"bob\"}");
    assert!(s.ends_with('\n'));
}

#[test]
fn ndjson_empty_list_is_empty() {
    assert_eq!(ndjson::write_list(&IndexMap::new()), "");
}

#[test]
fn ndjson_record_is_a_single_line() {
    let r = record(&[("x", int(1))]);
    assert_eq!(ndjson::write_record("a", &r), "{\"_id\":\"a\",\"x\":1}\n");
}

#[test]
fn ndjson_scalar_is_a_single_line() {
    assert_eq!(ndjson::write_scalar("count", &int(5)), "{\"count\":5}\n");
}
