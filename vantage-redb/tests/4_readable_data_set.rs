//! Test 4: ReadableValueSet for Table<Redb, EmptyEntity> against a seeded
//! tempfile-backed database. Each test creates its own DB so they don't
//! share state.

use vantage_dataset::prelude::*;
use vantage_redb::operation::RedbOperation;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::pagination::Pagination;
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

fn product_table(db: Redb) -> Table<Redb, EmptyEntity> {
    Table::<Redb, EmptyEntity>::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column(Column::<i64>::new("calories").with_flag(ColumnFlag::Indexed))
}

async fn seeded_table() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = product_table(db);

    table
        .insert_value(
            "flux_cupcake",
            &product_record("Flux Capacitor Cupcake", 120, 300),
        )
        .await
        .unwrap();
    table
        .insert_value(
            "delorean_donut",
            &product_record("DeLorean Doughnut", 135, 250),
        )
        .await
        .unwrap();
    table
        .insert_value("time_tart", &product_record("Time Tart", 220, 200))
        .await
        .unwrap();

    (path, table)
}

fn never_written_table() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    (path, product_table(db))
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
async fn test_list_of_a_table_never_written_is_empty() {
    let (_tmp, table) = never_written_table();

    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_product_by_id() {
    let (_tmp, table) = seeded_table().await;

    let record = table
        .get_value("delorean_donut")
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

    let result = table.get_value("nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_in_a_table_never_written_returns_none() {
    let (_tmp, table) = never_written_table();

    assert!(table.get_value("anything").await.unwrap().is_none());
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

    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("name".into(), AnyRedbType::new("seed".to_string()));
    table.insert_value("seed", &r).await.unwrap();
    WritableValueSet::delete(&table, "seed").await.unwrap();

    let result = table.get_some_value().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_reads_honour_conditions() {
    let (_tmp, mut table) = seeded_table().await;
    table.add_condition(table["calories"].clone().eq(250i64));

    let rows = table.list_values().await.unwrap();
    let (some_id, _) = table.get_some_value().await.unwrap().expect("a match");
    let count = table.data_source().get_table_count(&table).await.unwrap();

    assert_eq!(rows.into_keys().collect::<Vec<_>>(), ["delorean_donut"]);
    assert_eq!(some_id, "delorean_donut");
    assert_eq!(count, 1);
    assert!(table.get_value("delorean_donut").await.unwrap().is_some());
    assert!(table.get_value("flux_cupcake").await.unwrap().is_none());
}

#[tokio::test]
async fn test_count_unconditional() {
    let (_tmp, table) = seeded_table().await;
    let n = table.data_source().get_table_count(&table).await.unwrap();
    assert_eq!(n, 3);
}

#[tokio::test]
async fn test_count_of_a_table_never_written_is_zero() {
    let (_tmp, table) = never_written_table();

    let n = table.data_source().get_table_count(&table).await.unwrap();

    assert_eq!(n, 0);
}

#[tokio::test]
async fn test_pagination_pages_partition_the_rows() {
    let (_tmp, table) = seeded_table().await;
    let mut page1 = table.clone();
    page1.set_pagination(Some(Pagination::new(1, 2)));
    let mut page2 = table.clone();
    page2.set_pagination(Some(Pagination::new(2, 2)));

    let first = page1.list_values().await.unwrap();
    let last = page2.list_values().await.unwrap();

    assert_eq!(first.len(), 2);
    assert_eq!(last.len(), 1);
    let mut seen: Vec<String> = first.keys().chain(last.keys()).cloned().collect();
    seen.sort();
    assert_eq!(seen, ["delorean_donut", "flux_cupcake", "time_tart"]);
}

#[tokio::test]
async fn test_search_is_unsupported() {
    let (_tmp, mut table) = seeded_table().await;
    table.add_search("cupcake");

    let err = table.list_values().await.expect_err("redb has no search");

    assert!(err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn test_sum_max_min_return_errors() {
    let (_tmp, table) = seeded_table().await;
    let db = table.data_source();
    let price = db.create_column::<AnyRedbType>("price");

    assert!(db.get_table_sum(&table, &price).await.is_err());
    assert!(db.get_table_max(&table, &price).await.is_err());
    assert!(db.get_table_min(&table, &price).await.is_err());
}
