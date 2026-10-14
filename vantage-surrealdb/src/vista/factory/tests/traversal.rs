//! Reference traversal targets and the join conditions they carry.

use vantage_types::EmptyEntity;
use vantage_vista::VistaFactory;

use super::{hill_valley, parse, registry_resolver, test_db};
use crate::types::AnySurrealType;
use crate::vista::SurrealVistaFactory;
use crate::vista::factory::build_surreal_table;

#[test]
fn has_many_traversal_condition_from_thing_and_string_ids() {
    let batch_yaml = r#"
name: batch
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
references:
  tags:
    table: tag
    kind: has_many
    foreign_key: batch
"#;
    let tag_yaml = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  batch: { type: string }
"#;
    let batch_spec = parse(batch_yaml);
    let tag_spec = parse(tag_yaml);
    let resolver = registry_resolver(vec![("tag".to_string(), tag_spec)]);
    let table = build_surreal_table(&batch_spec, test_db(), Some(resolver)).expect("build batch");

    // Parent row with a Tag(8) record id (what SurrealDB actually returns).
    let thing_cbor = ciborium::Value::Tag(
        8,
        Box::new(ciborium::Value::Array(vec![
            ciborium::Value::Text("batch".into()),
            ciborium::Value::Text("0jz7".into()),
        ])),
    );
    let mut row: vantage_types::Record<AnySurrealType> = vantage_types::Record::new();
    row.insert("id".into(), AnySurrealType::from(thing_cbor));
    let target = table
        .get_ref_from_row::<EmptyEntity>("tags", &row)
        .expect("traverse");
    let q = target.select().preview();
    assert!(
        q.contains("batch = batch:0jz7"),
        "Thing id must render as a record literal, got: {q}"
    );

    // Parent row with the id as a plain string (e.g. re-parsed from text).
    let mut row: vantage_types::Record<AnySurrealType> = vantage_types::Record::new();
    row.insert(
        "id".into(),
        AnySurrealType::from(ciborium::Value::Text("batch:0jz7".into())),
    );
    let target = table
        .get_ref_from_row::<EmptyEntity>("tags", &row)
        .expect("traverse");
    let q = target.select().preview();
    assert!(
        q.contains("batch = batch:0jz7"),
        "string id must be coerced to a record literal, got: {q}"
    );
    // The insert invariant must carry the coerced record id too — a child
    // inserted into this set stores a link, not a string FK.
    let invariant = target.invariants().get("batch").expect("batch invariant");
    assert!(
        matches!(invariant.value(), ciborium::Value::Tag(8, _)),
        "invariant must hold a Tag(8) record id, got: {:?}",
        invariant.value()
    );
}

#[test]
fn has_one_traversal_coerces_string_fk_to_record_literal() {
    let tag_yaml = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  batch: { type: string }
references:
  batch:
    table: batch
    kind: has_one
    foreign_key: batch
"#;
    let batch_yaml = r#"
name: batch
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
"#;
    let tag_spec = parse(tag_yaml);
    let batch_spec = parse(batch_yaml);
    let resolver = registry_resolver(vec![("batch".to_string(), batch_spec)]);
    let table = build_surreal_table(&tag_spec, test_db(), Some(resolver)).expect("build tag");

    // FK arriving in string form (e.g. a row that round-tripped through
    // a script) must narrow the target by record literal, not by string.
    let mut row: vantage_types::Record<AnySurrealType> = vantage_types::Record::new();
    row.insert(
        "batch".into(),
        AnySurrealType::from(ciborium::Value::Text("batch:0jz7".into())),
    );
    let target = table
        .get_ref_from_row::<EmptyEntity>("batch", &row)
        .expect("traverse");
    let q = target.select().preview();
    assert!(
        q.contains("id = batch:0jz7"),
        "string FK must be coerced to a record literal, got: {q}"
    );
}

#[test]
fn resolver_supplies_child_columns_on_traversal() {
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
"#;
    let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
  bakery: { type: thing }
"#;
    let bakery_spec = parse(bakery_yaml);
    let product_spec = parse(product_yaml);

    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec.clone()),
        ("product".into(), product_spec.clone()),
    ]);

    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    let child = bakery
        .get_ref("products", Some(&hill_valley()))
        .expect("traverse products");

    let mut cols: Vec<&str> = child.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
    assert_eq!(child.id_column(), Some("id"));
    assert!(child.is_narrowed(), "the join is recorded on the target");
}

/// An unresolved target is a column-less table. Its bare form still
/// builds, but it declares no join column, so narrowing it to a row is
/// an error naming the relation.
#[test]
fn resolver_miss_gives_a_column_less_target() {
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  ghosts:
    table: missing_table
    kind: has_many
    foreign_key: bakery
"#;
    let bakery_spec = parse(bakery_yaml);
    let resolver = registry_resolver(vec![]);
    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build");

    let bare = bakery.get_ref("ghosts", None).expect("bare target");
    assert!(bare.columns().is_empty());

    let err = bakery
        .get_ref("ghosts", Some(&hill_valley()))
        .map(drop)
        .expect_err("no column");
    let text = format!("{err:?}");
    assert!(
        text.contains("ghosts") && text.contains("bakery"),
        "the error names the relation and column: {text}"
    );
}
