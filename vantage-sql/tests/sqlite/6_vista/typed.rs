//! A typed `Table<SqliteDB, _>` as a vista: reads, operator push-down,
//! computed columns, capabilities, narrowing clones, preview and writes.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{Record, entity};
use vantage_vista::{Capability, FilterOp};

use super::{TestResult, product_table, setup};

#[tokio::test]
async fn vista_lists_typed_sqlite_as_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.id_column(), Some("id"));

    // Clearing a search that was never set succeeds and changes nothing.
    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    assert_eq!(alpha.get("price"), Some(&CborValue::Integer(10i64.into())));
    Ok(())
}

#[tokio::test]
async fn vista_pushes_operator_conditions_server_side() -> TestResult {
    // products: a=Alpha/10/0, b=Beta/20/1, c=Gamma/30/0.
    let db = setup().await;

    // capability advertised
    assert!(
        db.vista_factory()
            .from_table(product_table(db.clone()))?
            .capabilities()
            .has(Capability::FilterOps),
        "sqlite advertises operator push-down"
    );

    // `!=` — price != 20 excludes Beta → a, c.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.add_condition("price", FilterOp::Ne, CborValue::Integer(20i64.into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    // `>` — price > 15 → Beta, Gamma.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.add_condition("price", FilterOp::Gt, CborValue::Integer(15i64.into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("b") && rows.contains_key("c") && !rows.contains_key("a"));

    // `in` — id in {a, c}.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let set = CborValue::Array(vec![
        CborValue::Text("a".into()),
        CborValue::Text("c".into()),
    ]);
    vista.add_condition("id", FilterOp::InSet, set)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    // `not in` — id not in {b}.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let set = CborValue::Array(vec![CborValue::Text("b".into())]);
    vista.add_condition("id", FilterOp::NotInSet, set)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    Ok(())
}

#[tokio::test]
async fn vista_delete_of_a_missing_row_succeeds() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.delete("nope").await?;
    vista.delete("a").await?;
    assert!(vista.get_value("a").await?.is_none());
    vista.delete("a").await?;
    Ok(())
}

/// Regression: `with_expression` survives `from_table`. Before the shell was
/// generic over `E`, the factory called `into_entity::<EmptyEntity>` which
/// reset the expressions map, silently dropping any computed columns.
#[tokio::test]
async fn vista_preserves_with_expression_columns() -> TestResult {
    let db = setup().await;

    #[entity(SqliteType)]
    #[derive(Debug, Clone, PartialEq, Default)]
    struct Product {
        name: String,
        price: i64,
    }

    let table = Table::<SqliteDB, Product>::new("product", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
        .with_expression("price_doubled", |_| sqlite_expr!("\"price\" * 2"));

    let vista = db.vista_factory().from_table(table)?;

    let rows = vista.list_values().await?;
    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("price_doubled"),
        Some(&CborValue::Integer(20i64.into())),
        "computed column should appear in vista output"
    );
    let gamma = rows.get("c").expect("row c");
    assert_eq!(
        gamma.get("price_doubled"),
        Some(&CborValue::Integer(60i64.into()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.has(Capability::Insert));
    assert!(caps.has(Capability::Update));
    assert!(caps.has(Capability::Delete));
    assert!(caps.has(Capability::ConfineWrites));
    assert!(caps.has(Capability::Order));
    assert!(caps.has(Capability::Search));
    assert!(caps.has(Capability::FilterOps));
    assert!(caps.has(Capability::Window));
    assert!(caps.has(Capability::Traverse));
    assert!(!caps.has(Capability::Subscribe));
    assert!(!caps.has(Capability::Cursor));
    assert!(!caps.has(Capability::Import));
    assert!(!caps.has(Capability::NativeConditions));
    Ok(())
}

/// A clone of a narrowed vista carries the narrowing; narrowing the clone
/// leaves the original alone.
#[tokio::test]
async fn vista_clone_owns_its_narrowing() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let cheap =
        vista
            .clone()
            .with_condition("price", FilterOp::Lt, CborValue::Integer(25i64.into()))?;
    let alpha = cheap.clone().with_search("alpha")?;

    assert_eq!(vista.count().await?, 3);
    assert_eq!(cheap.count().await?, 2);
    assert_eq!(alpha.count().await?, 1);
    assert_eq!(alpha.query().search.as_deref(), Some("alpha"));
    assert!(cheap.query().search.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_preview_renders_the_select() -> TestResult {
    let db = setup().await;
    let vista = db
        .vista_factory()
        .from_table(product_table(db.clone()))?
        .with_condition("price", FilterOp::Gt, CborValue::Integer(15i64.into()))?;
    let preview = vista.preview();
    assert_eq!(vista.driver_name(), "sqlite");
    // Every condition is pushed into the SELECT, so nothing is left to note.
    assert_eq!(preview.note, None);
    let sql = preview.query.expect("a SELECT");
    assert!(sql.contains("product") && sql.contains("15"), "{sql}");
    Ok(())
}

#[tokio::test]
async fn vista_upsert_creates_then_replaces() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let row = |name: &str| -> Record<CborValue> {
        [
            ("name", CborValue::Text(name.into())),
            ("price", CborValue::Integer(5i64.into())),
            ("is_deleted", CborValue::Bool(false)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    };
    vista.upsert_value("z", &row("Zeta")).await?;
    vista.upsert_value("z", &row("Zeta 2")).await?;
    let stored = vista.get_value("z").await?.expect("upserted");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Zeta 2".into())));
    assert_eq!(vista.count().await?, 4);
    Ok(())
}

#[tokio::test]
async fn vista_columns_advertise_orderable_flag() -> TestResult {
    use vantage_vista::ColumnFlag::Orderable;
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    assert!(vista.column("id").expect("id").has_flag(Orderable));
    assert!(vista.column("name").expect("name").has_flag(Orderable));
    assert!(vista.column("price").expect("price").has_flag(Orderable));
    assert!(
        vista
            .column("is_deleted")
            .expect("is_deleted")
            .has_flag(Orderable)
    );
    Ok(())
}
