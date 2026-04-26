//! Test 4: ReadableValueSet for Table<Redb, EmptyEntity> against a seeded
//! tempfile-backed database. Each test creates its own DB so they don't
//! share state.

use vantage_dataset::prelude::*;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

fn product_record(name: &str, price: i64, calories: i64) -> Record<AnyRedbType> {
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("name".into(), AnyRedbType::new(name.to_string()));
    r.insert("price".into(), AnyRedbType::new(price));
    r.insert("calories".into(), AnyRedbType::new(calories));
    r
}

async fn seeded_table() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<i64>("calories");

    table
        .insert_value(
            &"flux_cupcake".to_string(),
            &product_record("Flux Capacitor Cupcake", 120, 300),
        )
        .await
        .unwrap();
    table
        .insert_value(
            &"delorean_donut".to_string(),
            &product_record("DeLorean Doughnut", 135, 250),
        )
        .await
        .unwrap();
    table
        .insert_value(
            &"time_tart".to_string(),
            &product_record("Time Tart", 220, 200),
        )
        .await
        .unwrap();

    (path, table)
}

#[tokio::test]
async fn test_list_products() {
    let (_tmp, table) = seeded_table().await;

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 3);

    let cupcake = &values["flux_cupcake"];
    assert_eq!(
        cupcake["name"].try_get::<String>(),
        Some("Flux Capacitor Cupcake".into())
    );
    assert_eq!(cupcake["price"].try_get::<i64>(), Some(120));
    assert_eq!(cupcake["calories"].try_get::<i64>(), Some(300));
}

#[tokio::test]
async fn test_get_product_by_id() {
    let (_tmp, table) = seeded_table().await;

    let record = table
        .get_value(&"delorean_donut".to_string())
        .await
        .unwrap()
        .expect("delorean_donut exists");
    assert_eq!(
        record["name"].try_get::<String>(),
        Some("DeLorean Doughnut".into())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(135));
}

#[tokio::test]
async fn test_get_missing_id_returns_none() {
    let (_tmp, table) = seeded_table().await;

    let result = table.get_value(&"nonexistent".to_string()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_some_returns_arbitrary_row() {
    let (_tmp, table) = seeded_table().await;

    let result = table.get_some_value().await.unwrap();
    assert!(result.is_some());

    let (id, record) = result.unwrap();
    assert!(!id.is_empty());
    assert!(record.get("name").is_some());
    assert!(record.get("price").is_some());
}

#[tokio::test]
async fn test_get_some_on_empty_table_returns_none() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("empty", db)
        .with_id_column("id")
        .with_column_of::<String>("name");

    // Insert and immediately delete to force the table to be created but
    // empty (redb refuses to scan a never-opened table).
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("name".into(), AnyRedbType::new("seed".to_string()));
    table.insert_value(&"seed".to_string(), &r).await.unwrap();
    WritableValueSet::delete(&table, &"seed".to_string())
        .await
        .unwrap();

    let result = table.get_some_value().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_count_unconditional() {
    let (_tmp, table) = seeded_table().await;
    let n = table.data_source().get_table_count(&table).await.unwrap();
    assert_eq!(n, 3);
}

#[tokio::test]
async fn test_pagination_skip_and_limit() {
    let (_tmp, table) = seeded_table().await;

    let mut paged = table.clone();
    paged.set_pagination(Some(vantage_table::pagination::Pagination::new(2, 1)));

    let rows = paged.list_values().await.unwrap();
    // page=2 with 1 item per page → skip 1, take 1
    assert_eq!(rows.len(), 1);
}
