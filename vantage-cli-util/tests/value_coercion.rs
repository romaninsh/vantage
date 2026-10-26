//! CLI value coercion: auto-detect, `#`-typed JSON, value lists, and
//! column-typed coercion against a Vista's declared column types.

use ciborium::Value as CborValue;
use vantage_cli_util::vista_cli::{
    auto_detect, coerce_for_column, json_to_cbor, parse_value, parse_value_list,
};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn typed_vista() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("path_name", "alloc::string::String"))
        .with_column(Column::new("age", "i64"))
        .with_column(Column::new("count", "u32"))
        .with_column(Column::new("ratio", "f64"))
        .with_column(Column::new("active", "bool"))
        .with_column(Column::new("blob", "Mystery"))
        .with_id_column("id");
    Vista::new("things", MockShell::new().with_metadata(metadata))
}

// ─── Auto-detect ───────────────────────────────────────────────────────────

#[test]
fn auto_detect_prefers_integer_then_float_then_bool_then_text() {
    assert_eq!(auto_detect("42"), int(42));
    assert_eq!(auto_detect("-7"), int(-7));
    assert_eq!(auto_detect("2.5"), CborValue::Float(2.5));
    assert_eq!(auto_detect("true"), CborValue::Bool(true));
    assert_eq!(auto_detect("false"), CborValue::Bool(false));
    assert_eq!(auto_detect("hello"), text("hello"));
    assert_eq!(auto_detect(""), text(""));
}

// ─── Typed JSON ────────────────────────────────────────────────────────────

#[test]
fn parse_value_without_hash_autodetects() {
    assert_eq!(parse_value("42").unwrap(), int(42));
    assert_eq!(parse_value("true").unwrap(), CborValue::Bool(true));
    assert_eq!(parse_value("abc").unwrap(), text("abc"));
}

#[test]
fn hash_prefix_parses_a_json_literal() {
    assert_eq!(parse_value("#true").unwrap(), CborValue::Bool(true));
    assert_eq!(parse_value("#42").unwrap(), int(42));
    assert_eq!(parse_value("#1.5").unwrap(), CborValue::Float(1.5));
    assert_eq!(parse_value("#null").unwrap(), CborValue::Null);
}

#[test]
fn hash_prefix_keeps_a_number_lookalike_as_text() {
    assert_eq!(parse_value("#\"42\"").unwrap(), text("42"));
}

#[test]
fn hash_prefix_builds_arrays_and_maps() {
    assert_eq!(
        parse_value("#[1,\"two\",null]").unwrap(),
        CborValue::Array(vec![int(1), text("two"), CborValue::Null])
    );
    assert_eq!(
        parse_value("#{\"k\":\"v\",\"n\":1}").unwrap(),
        CborValue::Map(vec![(text("k"), text("v")), (text("n"), int(1))])
    );
}

#[test]
fn hash_prefix_with_invalid_json_is_an_error() {
    assert!(parse_value("#{not json").is_err());
    assert!(parse_value("#").is_err());
}

#[test]
fn json_to_cbor_maps_each_json_type_to_its_cbor_type() {
    let json = serde_json::json!({"a": [1, 2.5, null, true, "s"]});
    assert_eq!(
        json_to_cbor(json),
        CborValue::Map(vec![(
            text("a"),
            CborValue::Array(vec![
                int(1),
                CborValue::Float(2.5),
                CborValue::Null,
                CborValue::Bool(true),
                text("s"),
            ])
        )])
    );
}

// ─── Value lists ───────────────────────────────────────────────────────────

#[test]
fn value_list_splits_on_commas_and_autodetects_each_element() {
    assert_eq!(
        parse_value_list("1, two ,true").unwrap(),
        vec![int(1), text("two"), CborValue::Bool(true)]
    );
}

#[test]
fn value_list_skips_empty_elements() {
    assert_eq!(
        parse_value_list("a,,b,").unwrap(),
        vec![text("a"), text("b")]
    );
    assert!(parse_value_list("").unwrap().is_empty());
}

#[test]
fn value_list_hash_prefix_reads_a_json_array() {
    assert_eq!(
        parse_value_list("#[1,\"x\"]").unwrap(),
        vec![int(1), text("x")]
    );
}

#[test]
fn value_list_hash_prefix_rejects_non_arrays_and_bad_json() {
    assert!(parse_value_list("#5").is_err());
    assert!(parse_value_list("#{\"k\":1}").is_err());
    assert!(parse_value_list("#[").is_err());
}

// ─── Column-typed coercion ─────────────────────────────────────────────────

#[test]
fn string_column_keeps_lookalikes_as_text_whatever_the_module_path() {
    let v = typed_vista();
    assert_eq!(coerce_for_column(&v, "name", "true").unwrap(), text("true"));
    assert_eq!(
        coerce_for_column(&v, "path_name", "42").unwrap(),
        text("42")
    );
}

#[test]
fn bool_column_accepts_true_and_false() {
    let v = typed_vista();
    assert_eq!(
        coerce_for_column(&v, "active", "true").unwrap(),
        CborValue::Bool(true)
    );
    assert_eq!(
        coerce_for_column(&v, "active", "false").unwrap(),
        CborValue::Bool(false)
    );
}

#[test]
fn bool_column_rejects_other_text() {
    let v = typed_vista();
    assert!(coerce_for_column(&v, "active", "yes").is_err());
}

#[test]
fn integer_columns_parse_integers() {
    let v = typed_vista();
    assert_eq!(coerce_for_column(&v, "age", "42").unwrap(), int(42));
    assert_eq!(coerce_for_column(&v, "count", "7").unwrap(), int(7));
}

#[test]
fn integer_column_rejects_a_typo() {
    let v = typed_vista();
    assert!(coerce_for_column(&v, "age", "abc").is_err());
    assert!(coerce_for_column(&v, "age", "1.5").is_err());
}

#[test]
fn float_column_parses_numbers() {
    let v = typed_vista();
    assert_eq!(
        coerce_for_column(&v, "ratio", "1.5").unwrap(),
        CborValue::Float(1.5)
    );
    assert_eq!(
        coerce_for_column(&v, "ratio", "2").unwrap(),
        CborValue::Float(2.0)
    );
}

#[test]
fn float_column_rejects_a_typo() {
    let v = typed_vista();
    assert!(coerce_for_column(&v, "ratio", "abc").is_err());
}

#[test]
fn unknown_column_falls_back_to_autodetect() {
    let v = typed_vista();
    assert_eq!(coerce_for_column(&v, "nope", "42").unwrap(), int(42));
    assert_eq!(
        coerce_for_column(&v, "nope", "true").unwrap(),
        CborValue::Bool(true)
    );
}

#[test]
fn unrecognised_column_type_falls_back_to_autodetect() {
    let v = typed_vista();
    assert_eq!(coerce_for_column(&v, "blob", "42").unwrap(), int(42));
    assert_eq!(coerce_for_column(&v, "blob", "abc").unwrap(), text("abc"));
}
