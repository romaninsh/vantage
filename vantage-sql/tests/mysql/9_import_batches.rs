//! Import across several statements: rollback and statement sizing.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_sql::mysql::MysqlDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use super::import::{MYSQL_URL, TestResult, batch, setup, sku_row, stored, teardown, vista};

#[tokio::test]
async fn import_rolls_back_on_server_error() -> TestResult {
    let (db, name) = setup("rollback").await;
    sqlx::query(&format!("CREATE UNIQUE INDEX sku_unique ON `{name}` (sku)"))
        .execute(db.pool())
        .await?;
    let v = vista(&db, &name);
    v.insert_value("existing".to_string(), &sku_row("existing"))
        .await?;
    let before = stored(&db, &name).await;

    let rows = batch(vec![
        ("a", sku_row("a")),
        ("b", sku_row("dup")),
        ("c", sku_row("dup")),
    ]);
    assert!(v.import_values(&rows).await.is_err());
    assert_eq!(stored(&db, &name).await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_rolls_back_across_chunks() -> TestResult {
    let (db, name) = setup("rollback_chunks").await;
    sqlx::query(&format!("CREATE UNIQUE INDEX sku_unique ON `{name}` (sku)"))
        .execute(db.pool())
        .await?;
    let v = vista(&db, &name);

    // The duplicate sits at the very end, past the first INSERT statement.
    let mut rows: IndexMap<String, Record<CborValue>> = (0..1200)
        .map(|i| (format!("r{i:04}"), sku_row(&format!("s{i}"))))
        .collect();
    rows.insert("dup".to_string(), sku_row("s0"));

    assert!(v.import_values(&rows).await.is_err());
    assert!(stored(&db, &name).await.is_empty());

    rows.shift_remove("dup");
    assert_eq!(v.import_values(&rows).await?, 1200);
    assert_eq!(stored(&db, &name).await.len(), 1200);

    teardown(&db, &name).await;
    Ok(())
}

/// 400 rows × 201 columns binds more parameters than one MySQL statement
/// allows (65,535), so the import must split by width, not row count alone.
#[tokio::test]
async fn import_wide_table_stays_under_parameter_limit() -> TestResult {
    let db = MysqlDB::connect(MYSQL_URL).await?;
    let name = "imp_wide";
    let columns: Vec<String> = (0..200).map(|i| format!("c{i}")).collect();
    sqlx::query(&format!("DROP TABLE IF EXISTS `{name}`"))
        .execute(db.pool())
        .await?;
    let defs: Vec<String> = columns.iter().map(|c| format!("`{c}` INT")).collect();
    sqlx::query(&format!(
        "CREATE TABLE `{name}` (id VARCHAR(64) PRIMARY KEY, {})",
        defs.join(", ")
    ))
    .execute(db.pool())
    .await?;
    let mut wide = Table::<MysqlDB, EmptyEntity>::new(name, db.clone()).with_id_column("id");
    for c in &columns {
        wide = wide.with_column_of::<i64>(c);
    }
    let v = db.vista_factory().from_table(wide)?;

    let rows: IndexMap<String, Record<CborValue>> = (0..400)
        .map(|i| {
            let record = columns
                .iter()
                .map(|c| (c.clone(), CborValue::Integer(i.into())))
                .collect();
            (format!("w{i:03}"), record)
        })
        .collect();
    assert_eq!(v.import_values(&rows).await?, 400);
    let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM `{name}`"))
        .fetch_one(db.pool())
        .await?;
    assert_eq!(count, 400);

    teardown(&db, name).await;
    Ok(())
}
