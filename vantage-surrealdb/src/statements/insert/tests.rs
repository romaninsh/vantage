use crate::statements::insert::SurrealInsert;
use crate::types::AnySurrealType;
use vantage_expressions::Expressive;

#[test]
fn test_basic_insert() {
    let insert = SurrealInsert::new("users")
        .with_field("name", "John".to_string())
        .with_field("age", 30i64);

    let rendered = insert.preview();
    assert!(rendered.starts_with("CREATE users SET"));
    assert!(rendered.contains("name = \"John\""));
    assert!(rendered.contains("age = 30"));
}

#[test]
fn test_insert_with_id() {
    let insert = SurrealInsert::new("users")
        .with_id("john")
        .with_field("name", "John".to_string());

    let rendered = insert.preview();
    assert!(rendered.starts_with("CREATE users:john SET"));
    assert!(rendered.contains("name = \"John\""));
}

#[test]
fn test_empty_insert() {
    let insert = SurrealInsert::new("users");
    assert_eq!(insert.preview(), "CREATE users");
}

#[test]
fn test_empty_insert_with_id() {
    let insert = SurrealInsert::new("users").with_id("john");
    assert_eq!(insert.preview(), "CREATE users:john");
}

#[test]
fn test_identifier_escaping() {
    let insert = SurrealInsert::new("SELECT").with_field("FROM", "value".to_string());

    let rendered = insert.preview();
    assert!(rendered.contains("CREATE ⟨SELECT⟩"));
    assert!(rendered.contains("⟨FROM⟩ = \"value\""));
}

#[test]
fn test_insert_produces_parameterized_expression() {
    let insert = SurrealInsert::new("users")
        .with_field("name", "Alice".to_string())
        .with_field("age", 25i64);

    let expr = insert.expr();
    assert!(expr.template.contains("{}"));
    assert_eq!(expr.parameters.len(), 3); // target + 2 fields
}

#[test]
fn test_with_any_field() {
    let val = AnySurrealType::new(42i64);
    let insert = SurrealInsert::new("data").with_any_field("count", val);
    let rendered = insert.preview();
    assert!(rendered.contains("count = 42"));
}

#[test]
fn test_with_record() {
    let mut record = vantage_types::Record::new();
    record.insert("a".to_string(), AnySurrealType::new(1i64));
    record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

    let insert = SurrealInsert::new("t").with_id("1").with_record(&record);
    let p = insert.preview();
    assert!(p.contains("a = 1"));
    assert!(p.contains("b = \"hi\""));
}

#[test]
fn test_thing_field() {
    use crate::thing::Thing;
    let insert =
        SurrealInsert::new("order").with_field("customer", Thing::new("user", "alice"));

    let rendered = insert.preview();
    assert!(rendered.contains("CREATE order SET"));
}
