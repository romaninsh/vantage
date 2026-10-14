//! Writes through a vista: insert with and without an id, delete, upsert.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_types::Record;
use vantage_vista::FilterOp;

use super::{TestResult, product_table, setup};

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Delta".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value("delta", &record).await?;

    let fetched = vista.get_value("delta").await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Delta".into())));
    assert_eq!(
        fetched.get("price"),
        Some(&CborValue::Integer(99i64.into()))
    );

    vista.delete("delta").await?;
    assert!(vista.get_value("delta").await?.is_none());

    Ok(())
}

/// With no id given, SurrealDB makes one; the row is read back through the
/// (narrowed) set and returned as stored.
#[tokio::test]
async fn vista_insert_without_id_reads_back_the_stored_row() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db
        .vista_factory()
        .from_table(table)?
        .with_condition("price", FilterOp::Eq, 5)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Echo".into())),
        ("price".to_string(), CborValue::Integer(5i64.into())),
    ]
    .into_iter()
    .collect();
    let id = vista.insert_return_id_value(&record).await?;
    assert!(id.starts_with(&format!("{table_name}:")), "{id}");

    let all = db
        .vista_factory()
        .from_table(product_table(db.clone(), &table_name))?;
    let stored = all.get_value(&id).await?.expect("stored");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Echo".into())));
    Ok(())
}

#[tokio::test]
async fn vista_upsert_creates_then_replaces() -> TestResult {
    let (db, table_name) = setup().await;
    let vista = db
        .vista_factory()
        .from_table(product_table(db.clone(), &table_name))?;
    let named = |name: &str| -> Record<CborValue> {
        [("name".to_string(), CborValue::Text(name.into()))]
            .into_iter()
            .collect()
    };

    vista.upsert_value("echo", &named("First")).await?;
    vista.upsert_value("echo", &named("Second")).await?;
    let stored = vista.get_value("echo").await?.expect("upserted");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Second".into())));
    assert_eq!(vista.count().await?, 1);
    Ok(())
}
