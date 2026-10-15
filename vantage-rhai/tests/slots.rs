use std::collections::HashSet;

use serde::Deserialize;
use vantage_rhai::{Block, Expr, Template};

#[derive(Deserialize)]
struct ButtonDef {
    when: Option<Expr>,
    label: Template,
    action: Block,
}

#[test]
fn slots_deserialize_from_plain_yaml_scalars() {
    let def: ButtonDef = serde_yaml_ng::from_str(
        "when: row.ok\nlabel: 'Due ${ row.days } days'\naction: 'let n = 1; n'\n",
    )
    .unwrap();
    assert_eq!(def.when.unwrap().src(), "row.ok");
    assert_eq!(def.label.src(), "Due ${ row.days } days");
    assert_eq!(def.action.src(), "let n = 1; n");
}

#[test]
fn an_optional_slot_that_is_absent_deserializes_to_none() {
    let def: ButtonDef = serde_yaml_ng::from_str("label: hi\naction: x\n").unwrap();
    assert!(def.when.is_none());
}

#[test]
fn a_non_string_scalar_is_not_a_slot() {
    assert!(serde_json::from_str::<Expr>("[1, 2]").is_err());
    assert!(serde_json::from_str::<Block>("{\"a\": 1}").is_err());
}

#[test]
fn slots_serialize_back_out_as_the_same_scalar() {
    assert_eq!(
        serde_json::to_string(&Expr::from("a + b")).unwrap(),
        r#""a + b""#
    );
    assert_eq!(
        serde_json::to_string(&Template::from("Hi ${n}")).unwrap(),
        r#""Hi ${n}""#
    );
    assert_eq!(
        serde_json::to_string(&Block::from("let a = 1;")).unwrap(),
        r#""let a = 1;""#
    );
}

#[test]
fn a_slot_derefs_to_its_source_text() {
    let expr = Expr::from("row.total > 100");
    assert_eq!(expr.len(), 15);
    assert!(expr.starts_with("row."));
    let as_ref: &str = expr.as_ref();
    assert_eq!(as_ref, "row.total > 100");
}

#[test]
fn a_slot_displays_as_its_source() {
    assert_eq!(
        Template::from("Due ${ d } days").to_string(),
        "Due ${ d } days"
    );
    assert_eq!(format!("{}", Block::from("let a = 1;")), "let a = 1;");
}

#[test]
fn a_slot_compares_equal_to_a_str() {
    let expr = Expr::from("a + b");
    assert!(expr == "a + b");
    assert!(expr != "a - b");
    assert!(expr == *"a + b");
}

#[test]
fn a_slot_converts_from_str_and_string() {
    assert_eq!(Expr::from("x"), Expr::from(String::from("x")));
}

#[test]
fn a_default_slot_is_empty() {
    assert_eq!(Expr::default().src(), "");
    assert_eq!(Template::default().src(), "");
    assert_eq!(Block::default().src(), "");
}

#[test]
fn equal_slots_hash_alike() {
    let mut seen = HashSet::new();
    seen.insert(Expr::from("a"));
    seen.insert(Expr::from("a"));
    seen.insert(Expr::from("b"));
    assert_eq!(seen.len(), 2);
}

#[cfg(feature = "schema")]
mod schema {
    use schemars::schema_for;
    use vantage_rhai::{Block, Expr, Template};

    fn json_of<T: schemars::JsonSchema>() -> serde_json::Value {
        serde_json::to_value(schema_for!(T)).unwrap()
    }

    #[test]
    fn each_slot_schema_is_a_string_marked_as_rhai() {
        for (schema, description) in [
            (json_of::<Expr>(), Expr::DESCRIPTION),
            (json_of::<Template>(), Template::DESCRIPTION),
            (json_of::<Block>(), Block::DESCRIPTION),
        ] {
            assert_eq!(schema["type"], "string", "{schema}");
            assert_eq!(schema["x-language"], "rhai", "{schema}");
            assert_eq!(schema["description"], description, "{schema}");
            assert!(!description.is_empty());
        }
    }

    #[test]
    fn the_three_kinds_describe_themselves_differently() {
        assert_ne!(Expr::DESCRIPTION, Template::DESCRIPTION);
        assert_ne!(Expr::DESCRIPTION, Block::DESCRIPTION);
        assert_ne!(Template::DESCRIPTION, Block::DESCRIPTION);
    }
}
