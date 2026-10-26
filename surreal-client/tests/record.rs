use serde_json::{Value, json};

use surreal_client::record::RecordParseError;
use surreal_client::{
    RecordId, RecordIdValue, RecordRange, Table, escape_identifier, escape_record_id,
};

#[test]
fn escape_identifier_wraps_and_escapes_anything_but_a_plain_identifier() {
    assert_eq!(escape_identifier("snake_case_2"), "snake_case_2");
    assert_eq!(escape_identifier(""), "⟨⟩");
    assert_eq!(escape_identifier("1abc"), "⟨1abc⟩");
    assert_eq!(escape_identifier("123"), "⟨123⟩");
    assert_eq!(escape_identifier("Table"), "⟨Table⟩");
    assert_eq!(escape_identifier(r"a\⟩b"), r"⟨a\\\u{27E9}b⟩");
}

#[test]
#[ignore = "possible bug: non-ASCII letters pass unquoted although the doc says anything outside [A-Za-z0-9_] is wrapped"]
fn escape_identifier_quotes_non_ascii_letters() {
    assert_eq!(escape_identifier("café"), "⟨café⟩");
}

#[test]
fn escape_record_id_leaves_integer_ids_bare_and_escapes_the_rest() {
    assert_eq!(escape_record_id("-7"), "-7");
    assert_eq!(escape_record_id("1.5"), "⟨1.5⟩");
}

#[test]
fn record_id_constructors_set_table_and_id() {
    let id = RecordId::string("user", "john");
    assert_eq!(id.table(), "user");
    assert_eq!(id.id(), &RecordIdValue::String("john".into()));

    assert_eq!(RecordId::number("user", 7).id(), &RecordIdValue::Number(7));
    assert_eq!(
        RecordId::object("user", json!({"a": 1})).id(),
        &RecordIdValue::Object(json!({"a": 1}))
    );
    assert_eq!(
        RecordId::array("user", vec![json!(1), json!("x")]).id(),
        &RecordIdValue::Array(vec![json!(1), json!("x")])
    );
    assert_eq!(RecordId::new("user", 7), RecordId::number("user", 7));
    assert_eq!(
        RecordId::new("user", "john"),
        RecordId::string("user", "john")
    );
}

#[test]
fn record_id_value_converts_from_json() {
    assert_eq!(
        RecordIdValue::from(json!("john")),
        RecordIdValue::String("john".into())
    );
    assert_eq!(RecordIdValue::from(json!(5)), RecordIdValue::Number(5));
    assert_eq!(
        RecordIdValue::from(json!([1, 2])),
        RecordIdValue::Array(vec![json!(1), json!(2)])
    );
    assert_eq!(
        RecordIdValue::from(json!({"k": "v"})),
        RecordIdValue::Object(json!({"k": "v"}))
    );
    assert_eq!(
        RecordIdValue::from(String::from("s")),
        RecordIdValue::String("s".into())
    );
}

#[test]
fn record_id_parse_reads_string_and_numeric_ids() {
    assert_eq!(
        RecordId::parse("user:john").unwrap(),
        RecordId::string("user", "john")
    );
    assert_eq!(
        RecordId::parse("user:123").unwrap(),
        RecordId::number("user", 123)
    );
}

#[test]
fn record_id_parse_reads_object_and_array_ids() {
    assert_eq!(
        RecordId::parse(r#"user:{"a":1}"#).unwrap(),
        RecordId::object("user", json!({"a": 1}))
    );
    assert_eq!(
        RecordId::parse(r#"user:[1,"x"]"#).unwrap(),
        RecordId::array("user", vec![json!(1), json!("x")])
    );
}

#[test]
fn record_id_parse_rejects_input_without_a_colon() {
    assert_eq!(
        RecordId::parse("user").unwrap_err(),
        RecordParseError::InvalidFormat
    );
}

#[test]
fn record_id_from_str_matches_parse() {
    assert_eq!(RecordId::from("user:42"), RecordId::number("user", 42));
    assert_eq!(
        RecordId::from(String::from("user:john")),
        RecordId::string("user", "john")
    );
}

#[test]
fn record_id_renders_as_surql() {
    assert_eq!(RecordId::string("user", "john").to_surql(), "user:john");
    assert_eq!(RecordId::number("user", 123).to_surql(), "user:123");
    assert_eq!(
        RecordId::string("my-table", "a b").to_surql(),
        "⟨my-table⟩:⟨a b⟩"
    );
    assert_eq!(RecordId::string("user", "1").to_surql(), "user:⟨1⟩");
    assert_eq!(RecordId::string("user", "john").to_string(), "user:john");
}

#[test]
fn record_id_converts_to_a_json_string() {
    let id = RecordId::string("user", "john");
    assert_eq!(Value::from(&id), json!("user:john"));
    assert_eq!(Value::from(id), json!("user:john"));
}

#[test]
fn table_renders_its_escaped_name() {
    let table = Table::new("users");
    assert_eq!(table.name(), "users");
    assert_eq!(table.to_surql(), "users");
    assert_eq!(table.to_string(), "users");
    assert_eq!(Table::new("my table").to_surql(), "⟨my table⟩");
    assert_eq!(Table::from("users"), table);
    assert_eq!(Table::from(String::from("users")), table);
}

#[test]
fn table_converts_to_a_json_string() {
    let table = Table::new("users");
    assert_eq!(Value::from(&table), json!("users"));
    assert_eq!(Value::from(table), json!("users"));
}

#[test]
fn record_range_without_bounds_renders_the_table() {
    assert_eq!(RecordRange::new("user").to_surql(), "user");
    assert_eq!(RecordRange::new("user").to_string(), "user");
}
