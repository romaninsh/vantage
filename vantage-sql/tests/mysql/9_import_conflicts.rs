//! Import on narrowed vistas, and ids compared under the column collation.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_types::Record;

use super::import::{TestResult, batch, open_vista, row, setup, stored, teardown, vista};

#[tokio::test]
async fn import_conflicts_on_id_held_outside_the_set() -> TestResult {
    let (db, name) = setup("held_outside").await;
    vista(&db, &name)
        .insert_value("closed".to_string(), &row("closed one", "closed"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![
        ("new", row("a", "open")),
        ("closed", row("b", "open")),
    ]);
    let Err(err) = open_vista(&db, &name).import_values(&rows).await else {
        panic!("import must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_rejects_out_of_set_row() -> TestResult {
    let (db, name) = setup("out_of_set").await;
    vista(&db, &name)
        .insert_value("kept".to_string(), &row("kept", "open"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![
        ("a", row("a", "open")),
        ("b", row("b", "closed")),
        ("c", row("c", "open")),
    ]);
    let Err(err) = open_vista(&db, &name).import_values(&rows).await else {
        panic!("import must reject the out-of-set row");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_fills_set_columns_the_record_lacks() -> TestResult {
    let (db, name) = setup("fills_set").await;
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text("no status".into()));

    let rows = batch(vec![("a", record)]);
    assert_eq!(open_vista(&db, &name).import_values(&rows).await?, 1);

    let after = stored(&db, &name).await;
    assert_eq!(after[0].2.as_deref(), Some("open"));

    teardown(&db, &name).await;
    Ok(())
}

/// The default collation is case-insensitive: an incoming `A` is the stored
/// `a`, as a single insert's lookup finds it, so it is skipped untouched.
#[tokio::test]
async fn import_matches_ids_under_the_column_collation() -> TestResult {
    let (db, name) = setup("collation").await;
    vista(&db, &name)
        .insert_value("a".to_string(), &row("old", "open"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![("A", row("changed", "open"))]);
    assert_eq!(vista(&db, &name).import_values(&rows).await?, 0);
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_matches_ids_under_the_column_collation_in_a_set() -> TestResult {
    let (db, name) = setup("collation_set").await;
    vista(&db, &name)
        .insert_value("a".to_string(), &row("old", "open"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![("A", row("changed", "open"))]);
    assert_eq!(open_vista(&db, &name).import_values(&rows).await?, 0);
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}

/// Trailing spaces follow the collation's pad attribute: MySQL 8's default
/// `utf8mb4_0900_ai_ci` is NO PAD, so `a ` is a new id; MariaDB's defaults are
/// PAD SPACE, so `a ` is the stored `a` and is skipped.
#[tokio::test]
async fn import_matches_trailing_space_ids_by_pad_attribute() -> TestResult {
    let (db, name) = setup("collation_pad").await;
    vista(&db, &name)
        .insert_value("a".to_string(), &row("old", "open"))
        .await?;

    let rows = batch(vec![("a ", row("padded", "open"))]);
    let inserted = vista(&db, &name).import_values(&rows).await?;
    let expected = if super::is_mariadb(&db).await { 0 } else { 1 };
    assert_eq!(inserted, expected);
    assert_eq!(stored(&db, &name).await.len(), 1 + expected);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_conflicts_on_collation_equal_id_outside_the_set() -> TestResult {
    let (db, name) = setup("collation_outside").await;
    vista(&db, &name)
        .insert_value("a".to_string(), &row("old", "closed"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![("A", row("changed", "open"))]);
    let Err(err) = open_vista(&db, &name).import_values(&rows).await else {
        panic!("import must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}
