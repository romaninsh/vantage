//! Writes through a vista: insert with and without an id, delete, upsert.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_surrealdb::types::AnySurrealType;
use vantage_types::Record;
use vantage_vista::{FilterOp, VistaFactory};

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

    // SurrealDB makes only the record id, so an `auto` id elsewhere could
    // never be read back: the build refuses it.
    let auto = |column: &str| format!("name: {table_name}\ncolumns:\n  {column}\n");
    for column in [
        "code: { type: string, flags: [id, auto] }",
        "id: { type: string, flags: [id, auto], surreal: { field: code } }",
    ] {
        let err = db.vista_factory().from_yaml(&auto(column)).unwrap_err();
        assert!(
            err.to_string()
                .contains("auto id column must be the record id"),
            "{column}: {err}"
        );
    }
    db.vista_factory()
        .from_yaml(&auto("id: { type: string, flags: [id, auto] }"))?;
    Ok(())
}

/// Rhai's `now()` and the audit logger hand over `Tag(0, rfc3339)`, which
/// SurrealDB refuses; the vista writes it as a datetime, nested ones too.
#[tokio::test]
async fn vista_writes_tag0_datetimes_as_datetimes() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name)
        .with_column_of::<AnySurrealType>("ran_at")
        .with_column_of::<AnySurrealType>("at")
        .with_column_of::<AnySurrealType>("meta")
        .with_column_of::<AnySurrealType>("history");
    let vista = db.vista_factory().from_table(table)?;
    let tag0 = || CborValue::Tag(0, Box::new(CborValue::Text("2026-01-02T03:04:05Z".into())));
    let tag12 = CborValue::Tag(
        12,
        Box::new(CborValue::Array(vec![
            CborValue::Integer(1767323045i64.into()),
            CborValue::Integer(0i64.into()),
        ])),
    );
    let is_datetime = |stored: &Record<CborValue>, key: &str| {
        matches!(stored.get(key), Some(CborValue::Tag(12, _)))
    };

    let record: Record<CborValue> = [
        ("ran_at".to_string(), tag0()),
        (
            "meta".to_string(),
            CborValue::Map(vec![(CborValue::Text("at".into()), tag0())]),
        ),
        ("history".to_string(), CborValue::Array(vec![tag0()])),
    ]
    .into_iter()
    .collect();
    vista.insert_value("a", &record).await?;
    vista.upsert_value("b", &record).await?;
    vista
        .patch_value("a", &[("at".to_string(), tag0())].into_iter().collect())
        .await?;

    let a = vista.get_value("a").await?.expect("inserted");
    let b = vista.get_value("b").await?.expect("upserted");
    assert!(is_datetime(&a, "ran_at"), "{:?}", a.get("ran_at"));
    assert!(is_datetime(&a, "at"), "{:?}", a.get("at"));
    assert!(is_datetime(&b, "ran_at"), "{:?}", b.get("ran_at"));
    assert_eq!(
        a.get("meta"),
        Some(&CborValue::Map(vec![(
            CborValue::Text("at".into()),
            tag12.clone()
        )]))
    );
    assert_eq!(a.get("history"), Some(&CborValue::Array(vec![tag12])));
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
