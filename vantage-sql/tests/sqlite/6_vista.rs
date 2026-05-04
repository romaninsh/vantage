//! Vista integration: typed `Table<SqliteDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Uses in-memory SQLite — no external setup required.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

type TestResult = std::result::Result<(), Box<dyn Error>>;

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO product VALUES \
         ('a', 'Alpha', 10, 0), \
         ('b', 'Beta', 20, 1), \
         ('c', 'Gamma', 30, 0)",
    )
    .execute(db.pool())
    .await
    .unwrap();

    db
}

fn product_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

#[tokio::test]
async fn vista_lists_typed_sqlite_as_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.get_id_column(), Some("id"));

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
async fn vista_get_value_by_id() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let row = vista
        .get_value(&"b".to_string())
        .await?
        .expect("row b exists");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Beta".to_string())));

    let missing = vista.get_value(&"nope".to_string()).await?;
    assert!(missing.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a"));
    assert!(rows.contains_key("c"));
    assert!(!rows.contains_key("b"));
    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
sqlite:
  table: product
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.contains_key("a"));
    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Delta".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value(&"d".to_string(), &record).await?;

    let fetched = vista.get_value(&"d".to_string()).await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Delta".into())));

    vista.delete(&"d".to_string()).await?;
    assert!(vista.get_value(&"d".to_string()).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    assert!(!caps.can_subscribe);
    Ok(())
}
