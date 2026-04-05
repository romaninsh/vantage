use serde_json::Value as JsonValue;
use vantage_sql::sql_expr;
use vantage_sql::sqlite::statements::*;
use vantage_types::Record;

// -- SELECT -----------------------------------------------------------------

#[test]
fn test_select_all() {
    let s = SqliteSelect::new().from("users");
    assert_eq!(s.preview(), "SELECT * FROM \"users\"");
}

#[test]
fn test_select_fields() {
    let s = SqliteSelect::new()
        .from("users")
        .field("name")
        .field("age");
    assert_eq!(s.preview(), "SELECT \"name\", \"age\" FROM \"users\"");
}

#[test]
fn test_select_where() {
    let s = SqliteSelect::new()
        .from("users")
        .with_where(sql_expr!("\"age\" > {}", 18));
    assert_eq!(s.preview(), "SELECT * FROM \"users\" WHERE \"age\" > 18");
}

#[test]
fn test_select_order_by() {
    let s = SqliteSelect::new()
        .from("users")
        .with_order_by("name", true)
        .with_order_by("age", false);
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"users\" ORDER BY \"name\", \"age\" DESC"
    );
}

#[test]
fn test_select_limit_offset() {
    let s = SqliteSelect::new()
        .from("users")
        .with_limit(10)
        .with_skip(20);
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"users\" LIMIT 10 OFFSET 20"
    );
}

#[test]
fn test_select_distinct() {
    let s = SqliteSelect::new()
        .from("users")
        .field("name")
        .with_distinct();
    assert_eq!(s.preview(), "SELECT DISTINCT \"name\" FROM \"users\"");
}

#[test]
fn test_select_group_by() {
    let s = SqliteSelect::new()
        .from("orders")
        .field("bakery_id")
        .with_expression(sql_expr!("COUNT(*)"), Some("count".to_string()))
        .with_group_by("bakery_id");
    assert_eq!(
        s.preview(),
        "SELECT \"bakery_id\", COUNT(*) AS \"count\" FROM \"orders\" GROUP BY \"bakery_id\""
    );
}

#[test]
fn test_select_complex() {
    let s = SqliteSelect::new()
        .from("client")
        .field("name")
        .field("balance")
        .with_where(sql_expr!("\"is_paying_client\" = {}", true))
        .with_order_by("balance", false)
        .with_limit(5);
    assert_eq!(
        s.preview(),
        "SELECT \"name\", \"balance\" FROM \"client\" WHERE \"is_paying_client\" = true ORDER BY \"balance\" DESC LIMIT 5"
    );
}

// -- INSERT -----------------------------------------------------------------

#[test]
fn test_insert_fields() {
    let i = SqliteInsert::new("users")
        .with_field("name", JsonValue::String("Alice".into()))
        .with_field("age", JsonValue::Number(30.into()));
    assert_eq!(
        i.preview(),
        "INSERT INTO \"users\" (\"name\", \"age\") VALUES (\"Alice\", 30)"
    );
}

#[test]
fn test_insert_from_record() {
    let mut record = Record::new();
    record.insert("name".to_string(), JsonValue::String("Bob".into()));
    record.insert("active".to_string(), JsonValue::Bool(true));

    let i = SqliteInsert::new("users").with_record(&record);
    assert_eq!(
        i.preview(),
        "INSERT INTO \"users\" (\"name\", \"active\") VALUES (\"Bob\", true)"
    );
}

#[test]
fn test_insert_default() {
    let i = SqliteInsert::new("counters");
    assert_eq!(i.preview(), "INSERT INTO \"counters\" DEFAULT VALUES");
}

// -- UPDATE -----------------------------------------------------------------

#[test]
fn test_update_with_condition() {
    let u = SqliteUpdate::new("users")
        .with_field("name", JsonValue::String("Alice".into()))
        .with_condition(sql_expr!("\"id\" = {}", "marty"));
    assert_eq!(
        u.preview(),
        "UPDATE \"users\" SET \"name\" = \"Alice\" WHERE \"id\" = \"marty\""
    );
}

#[test]
fn test_update_multiple_fields() {
    let u = SqliteUpdate::new("users")
        .with_field("name", JsonValue::String("Alice".into()))
        .with_field("age", JsonValue::Number(31.into()))
        .with_condition(sql_expr!("\"id\" = {}", "marty"));
    assert_eq!(
        u.preview(),
        "UPDATE \"users\" SET \"name\" = \"Alice\", \"age\" = 31 WHERE \"id\" = \"marty\""
    );
}

#[test]
fn test_update_from_record() {
    let mut record = Record::new();
    record.insert(
        "balance".to_string(),
        JsonValue::Number(serde_json::Number::from_f64(200.0).unwrap()),
    );

    let u = SqliteUpdate::new("client")
        .with_record(&record)
        .with_condition(sql_expr!("\"id\" = {}", "doc"));
    assert_eq!(
        u.preview(),
        "UPDATE \"client\" SET \"balance\" = 200.0 WHERE \"id\" = \"doc\""
    );
}

// -- DELETE -----------------------------------------------------------------

#[test]
fn test_delete_all() {
    let d = SqliteDelete::new("users");
    assert_eq!(d.preview(), "DELETE FROM \"users\"");
}

#[test]
fn test_delete_with_condition() {
    let d =
        SqliteDelete::new("users").with_condition(sql_expr!("\"id\" = {}", "biff"));
    assert_eq!(
        d.preview(),
        "DELETE FROM \"users\" WHERE \"id\" = \"biff\""
    );
}

#[test]
fn test_delete_multiple_conditions() {
    let d = SqliteDelete::new("product")
        .with_condition(sql_expr!("\"is_deleted\" = {}", true))
        .with_condition(sql_expr!("\"price\" < {}", 100));
    assert_eq!(
        d.preview(),
        "DELETE FROM \"product\" WHERE \"is_deleted\" = true AND \"price\" < 100"
    );
}
