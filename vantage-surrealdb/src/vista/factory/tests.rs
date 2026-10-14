//! Factory behaviour over a mock engine: spec lowering, metadata, and
//! reference traversal targets. Nothing here talks to a server.
//!
//! - this file: the fixtures, traversal idioms and spec columns;
//! - `metadata`: a spec's references in the vista metadata;
//! - `traversal`: reference traversal targets and their join conditions;
//! - `scripts`: scripted references and `modify:` scripts.

mod metadata;
mod scripts;
mod traversal;

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_types::Record;
use vantage_vista::ColumnFlag;

use super::*;

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

fn registry_resolver(
    specs: Vec<(String, SurrealVistaSpec)>,
) -> Arc<dyn SpecResolver<SurrealVistaSpec>> {
    let map: IndexMap<String, SurrealVistaSpec> = specs.into_iter().collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

/// A parent bakery row whose id arrives as text.
fn hill_valley() -> Record<CborValue> {
    let mut row: Record<CborValue> = Record::new();
    row.insert("id".into(), CborValue::Text("bakery:hill_valley".into()));
    row
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
