//! A contained relation over a JSON host column.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::ReferenceKind;

use super::{PG_URL, TestResult};

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

/// A `cart` whose `items` column is a JSON array (TEXT), surfaced as a
/// contains-many relation — same JSON-blob round-trip as SQLite.
fn cart_table(db: PostgresDB, table: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(table, db)
        .with_id_column("id")
        .with_column_of::<String>("items")
        .with_contained_many(
            "items",
            "items",
            |db| {
                Table::new("items", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

#[tokio::test]
async fn contained_json_column_round_trips_on_postgres() -> TestResult {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let t = "vista_cart_contained";
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\""))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE \"{t}\" (id TEXT PRIMARY KEY, items TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        r#"INSERT INTO "{t}" VALUES ('c1', '[{{"sku":"a","qty":1}},{{"sku":"b","qty":2}}]')"#
    ))
    .execute(db.pool())
    .await
    .unwrap();

    let vista = db.vista_factory().from_table(cart_table(db.clone(), t))?;
    assert_eq!(
        vista.references()["items"].kind,
        ReferenceKind::ContainsMany
    );

    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 2);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    assert_eq!(items2.list_values().await?.len(), 3);

    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", Some(&cart3))?;
    assert_eq!(
        items3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );

    sqlx::query(&format!("DROP TABLE \"{t}\""))
        .execute(db.pool())
        .await
        .ok();
    Ok(())
}
