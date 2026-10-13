//! Factory behaviour over a mock engine: spec lowering, metadata, and
//! reference traversal targets. Nothing here talks to a server.

use super::*;
use indexmap::IndexMap;
use std::sync::Arc;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_vista::{ColumnFlag, JoinKey, ReferenceKind};

fn test_db() -> SurrealDB {
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test".into()),
        Some("test".into()),
    );
    SurrealDB::new(client)
}

fn parse(yaml: &str) -> SurrealVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("yaml parse")
}

fn registry_resolver(specs: Vec<(String, SurrealVistaSpec)>) -> SpecResolver<SurrealVistaSpec> {
    let map: IndexMap<String, SurrealVistaSpec> = specs.into_iter().collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

/// SurrealDB lowers an implicit reference into a native idiom path, each
/// segment escaped on its own so the record links are traversed
/// (`batch.golf_course.name`) rather than looked up as one literal field.
#[test]
fn traversal_path_expr_builds_escaped_idiom_path() {
    use vantage_table::traits::table_source::TableSource;
    let db = test_db();
    assert!(db.supports_traversal());

    let one = db.traversal_path_expr(&["batch"], "name").unwrap();
    assert_eq!(one.preview(), "batch.name");

    let two = db
        .traversal_path_expr(&["batch", "golf_course"], "name")
        .unwrap();
    assert_eq!(two.preview(), "batch.golf_course.name");

    // A reserved keyword segment is escaped independently — not the whole
    // path — so traversal survives.
    let reserved = db.traversal_path_expr(&["SELECT"], "name").unwrap();
    assert_eq!(reserved.preview(), "⟨SELECT⟩.name");
}

/// A dotted spec column routes through the traversal import: the idiom
/// descends the relation's **link field** (here `batch`), not its registry
/// name (`owner`), the flat dotted alias is escaped as a single
/// identifier, and metadata flags the column `calculated` (not orderable —
/// the dotted name is a projection alias, not a sortable field).
#[test]
fn factory_routes_dotted_spec_column_through_traversal() {
    let batch_yaml = r#"
name: batch
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
"#;
    let tag_yaml = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  status: { type: string }
  batch: { type: string }
  owner.name: { type: string }
references:
  owner:
    table: batch
    kind: has_one
    foreign_key: batch
"#;
    let resolver = registry_resolver(vec![("batch".to_string(), parse(batch_yaml))]);
    let table =
        build_surreal_table(&parse(tag_yaml), test_db(), Some(resolver)).expect("build tag");

    let preview = table.select().preview();
    assert!(
        preview.contains("batch.name"),
        "idiom must descend the link field `batch`: {preview}"
    );
    assert!(
        preview.contains("⟨owner.name⟩"),
        "dotted alias must be a single escaped identifier: {preview}"
    );
    assert!(
        !preview.contains("(owner.name)"),
        "relation name must not be used as the idiom path: {preview}"
    );

    let metadata = metadata_from_table(&table);
    let col = metadata.columns.get("owner.name").expect("metadata column");
    assert!(
        col.has_flag(ColumnFlag::Calculated),
        "imported column must be flagged calculated: {:?}",
        col.flags()
    );
    assert!(
        !col.has_flag(ColumnFlag::Orderable),
        "imported column must not be orderable: {:?}",
        col.flags()
    );
    let status = metadata.columns.get("status").expect("status column");
    assert!(status.has_flag(ColumnFlag::Orderable));
}

/// A dotted spec column naming an unknown relation fails the build —
/// the factory propagates `with_active_columns`' build-time error instead
/// of leaving a dead literal column.
#[test]
fn factory_rejects_dotted_spec_column_with_unknown_relation() {
    let yaml = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  nope.name: { type: string }
"#;
    assert!(build_surreal_table(&parse(yaml), test_db(), None).is_err());
}

#[cfg(feature = "rhai")]
#[test]
fn expr_column_projects_into_select() {
    let yaml = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  batch: { type: string }
  batch_name: { type: string, expr: 'surreal::ident("batch")["name"]' }
"#;
    let spec = parse(yaml);
    let table = build_surreal_table(&spec, test_db(), None).expect("build");
    let preview = table.select().preview();
    assert!(
        preview.contains("batch.name") && preview.contains("AS batch_name"),
        "expr column must project as (batch.name) AS batch_name, got: {preview}"
    );
}

/// The WHERE clause runs before the select list is projected, so a
/// condition on an `expr:` column must carry the expression itself —
/// the `AS` alias is not a field there and would match nothing.
#[cfg(feature = "rhai")]
#[test]
fn conditions_on_expr_column_use_the_expression_not_the_alias() {
    use vantage_vista::{Condition, FilterOp, TableShell};

    let yaml = r#"
name: line
columns:
  id: { type: string, flags: [id] }
  qty: { type: int }
  price: { type: int }
  total: { type: int, expr: 'surreal::mul(surreal::ident("qty"), surreal::ident("price"))' }
"#;
    let table = build_surreal_table(&parse(yaml), test_db(), None).expect("build");
    let metadata = metadata_from_table(&table);
    let mut shell = SurrealTableShell::new(table, VistaCapabilities::default(), metadata, None);

    shell
        .add_condition(&Condition::new("total", FilterOp::Eq, 3))
        .expect("eq");
    shell
        .add_condition(&Condition::new("total", FilterOp::Gt, 6))
        .expect("gt");
    shell
        .add_condition(&Condition::new("total", FilterOp::Like, "1"))
        .expect("like");

    let preview = shell.table.select().preview();
    let (_, where_clause) = preview.split_once("WHERE").expect("WHERE clause");
    assert!(
        where_clause.matches("qty * price").count() >= 3,
        "every condition must use the expression, got: {preview}"
    );
    assert!(
        !where_clause.contains("total"),
        "the alias must not appear in WHERE, got: {preview}"
    );
}

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
fn build_from_spec_surfaces_references_in_metadata() {
    let yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
  primary_product:
    table: product
    kind: has_one
    foreign_key: primary_product
"#;
    let spec = parse(yaml);
    let factory = SurrealVistaFactory::new(test_db());
    let vista = factory.build_from_spec(spec).expect("build");

    let mut listed: Vec<&str> = vista.references().keys().map(String::as_str).collect();
    listed.sort();
    assert_eq!(listed, vec!["primary_product", "products"]);

    let products = vista.reference("products").expect("products ref");
    assert_eq!(products.target, "product");
    assert_eq!(products.kind, ReferenceKind::HasMany);
    assert_eq!(products.keys, vec![JoinKey::new("id", "bakery")]);

    let primary = vista.reference("primary_product").expect("primary ref");
    assert_eq!(primary.kind, ReferenceKind::HasOne);
    assert_eq!(primary.keys, vec![JoinKey::to_target_id("primary_product")]);
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

    let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
    row.insert(
        "id".into(),
        ciborium::Value::Text("bakery:hill_valley".into()),
    );

    let child = bakery
        .get_ref("products", Some(&row))
        .expect("traverse products");

    let mut cols: Vec<&str> = child.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
    assert_eq!(child.id_column(), Some("id"));
    assert!(child.is_narrowed(), "the join is recorded on the target");
}

#[cfg(feature = "rhai")]
#[test]
fn scripted_reference_builds_target_via_rhai() {
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").where("bakery", row.id)
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
        ("product".into(), product_spec),
    ]);

    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    // The build_script rode through the extras slot onto the Reference.
    let reference = bakery.reference("products").expect("products ref");
    assert!(reference.build_script.is_some());

    // Traversal evaluates the script: `table("product")` resolves a fresh
    // product Vista (proving the script path fired, not the FK path).
    let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
    row.insert(
        "id".into(),
        ciborium::Value::Text("bakery:hill_valley".into()),
    );
    let child = bakery
        .get_ref("products", Some(&row))
        .expect("scripted traverse products");

    let mut cols: Vec<&str> = child.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
    assert_eq!(child.name(), "product");
}

#[cfg(feature = "rhai")]
#[test]
fn scripted_reference_refuses_vendor_condition_on_table() {
    // `with_condition` needs a Vista in hand (`self`); on a handle from
    // `table(name)` it is an error naming the verb.
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").with_condition(surreal::ident("bakery") == row.id)
"#;
    let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  bakery: { type: thing }
"#;
    let bakery_spec = parse(bakery_yaml);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec.clone()),
        ("product".into(), parse(product_yaml)),
    ]);
    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
    row.insert(
        "id".into(),
        ciborium::Value::Text("bakery:hill_valley".into()),
    );
    let err = bakery
        .get_ref("products", Some(&row))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );
}

#[cfg(feature = "rhai")]
#[test]
fn modify_script_tweaks_built_vista_with_vendor_condition() {
    // The YAML builds a normal, writable `client` table; the `modify` script
    // then narrows it with an expression condition YAML can't express,
    // routed through `add_native_condition` under its rendered SurrealQL.
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  is_paying_client: { type: bool }
surreal:
  table: clients
  modify: |
    self.with_condition(surreal::ident("is_paying_client") == true)
       .sort("name", "asc")
"#;
    let spec = parse(yaml);
    let factory = SurrealVistaFactory::new(test_db());
    let vista = factory.build_from_spec(spec).expect("build + modify");

    // The base table stays writable (a condition narrows, it doesn't make
    // the vista read-only), and the schema is intact.
    assert!(vista.capabilities().has(Capability::Insert));
    let mut cols: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["id", "is_paying_client", "name"]);

    assert_eq!(vista.query().native, vec!["is_paying_client = true"]);
    assert_eq!(
        vista.query().order,
        Some(("name".to_string(), vantage_vista::SortDirection::Ascending))
    );
    let preview = vista.preview().query.expect("preview query");
    assert!(
        preview.contains("WHERE is_paying_client = true"),
        "{preview}"
    );
}

/// Without the script engine, a spec that carries a script fails to build,
/// naming the vista (and the relation, for a reference script), instead of
/// silently dropping the script.
#[cfg(not(feature = "rhai"))]
#[test]
fn scripts_without_rhai_fail_the_build() {
    let context = |err: &vantage_core::VantageError, key: &str| {
        err.context
            .get(key)
            .map(|v| v.trim_matches('"').to_string())
    };
    let with_ref_script = parse(
        r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: 'table("product")'
"#,
    );
    let err = SurrealVistaFactory::new(test_db())
        .build_from_spec(with_ref_script)
        .unwrap_err();
    assert_eq!(context(&err, "vista").as_deref(), Some("bakery"), "{err:?}");
    assert_eq!(
        context(&err, "relation").as_deref(),
        Some("products"),
        "{err:?}"
    );

    let with_modify = parse(
        r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
surreal:
  modify: 'self'
"#,
    );
    let err = SurrealVistaFactory::new(test_db())
        .build_from_spec(with_modify)
        .unwrap_err();
    assert_eq!(context(&err, "vista").as_deref(), Some("bakery"), "{err:?}");
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

    let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
    row.insert(
        "id".into(),
        ciborium::Value::Text("bakery:hill_valley".into()),
    );

    let bare = bakery.get_ref("ghosts", None).expect("bare target");
    assert!(bare.columns().is_empty());

    let err = bakery
        .get_ref("ghosts", Some(&row))
        .map(drop)
        .expect_err("no column");
    let text = format!("{err:?}");
    assert!(
        text.contains("ghosts") && text.contains("bakery"),
        "the error names the relation and column: {text}"
    );
}
