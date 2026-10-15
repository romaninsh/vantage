//! Test 4: WritableValueSet and InsertableValueSet for Table<Redb, EmptyEntity>.

use vantage_dataset::prelude::*;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

fn fresh_table(name: &str) -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    (path, table)
}

fn record(fields: &[(&str, AnyRedbType)]) -> Record<AnyRedbType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

async fn seed(table: &Table<Redb, EmptyEntity>) {
    table
        .insert_value(
            "a",
            &record(&[
                ("name", AnyRedbType::new("Alpha".to_string())),
                ("price", AnyRedbType::new(10i64)),
            ]),
        )
        .await
        .unwrap();
    table
        .insert_value(
            "b",
            &record(&[
                ("name", AnyRedbType::new("Beta".to_string())),
                ("price", AnyRedbType::new(20i64)),
            ]),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn test_insert() {
    let (_tmp, table) = fresh_table("ins");
    seed(&table).await;

    let rec = record(&[
        ("name", AnyRedbType::new("Gamma".to_string())),
        ("price", AnyRedbType::new(30i64)),
    ]);
    let result = table.insert_value("c", &rec).await.unwrap();
    assert_eq!(result["name"].try_get::<String>(), Some("Gamma".into()));
    assert_eq!(result["price"].try_get::<i64>(), Some(30));

    let fetched = table.get_value("c").await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Gamma".into()));
    assert_eq!(table.list_values().await.unwrap().len(), 3);
}

#[tokio::test]
async fn test_insert_of_an_existing_id_keeps_the_stored_row() {
    let (_tmp, table) = fresh_table("insidem");
    seed(&table).await;

    let other = record(&[
        ("name", AnyRedbType::new("Changed".to_string())),
        ("price", AnyRedbType::new(999i64)),
    ]);
    let returned = table.insert_value("a", &other).await.unwrap();

    assert_eq!(returned["name"].try_get::<String>(), Some("Alpha".into()));
    let stored = table.get_value("a").await.unwrap().expect("row exists");
    assert_eq!(stored["name"].try_get::<String>(), Some("Alpha".into()));
    assert_eq!(stored["price"].try_get::<i64>(), Some(10));
}

#[tokio::test]
async fn test_replace_overwrites_all_fields() {
    let (_tmp, table) = fresh_table("rep");
    seed(&table).await;

    let rec = record(&[("name", AnyRedbType::new("Alpha Replaced".to_string()))]);
    table.replace_value("a", &rec).await.unwrap();

    let fetched = table.get_value("a").await.unwrap().expect("row a exists");
    assert_eq!(
        fetched["name"].try_get::<String>(),
        Some("Alpha Replaced".into())
    );
    assert!(
        fetched.get("price").is_none(),
        "replace must drop fields the new record lacks"
    );
}

#[tokio::test]
async fn test_replace_creates_if_missing() {
    let (_tmp, table) = fresh_table("repmiss");
    seed(&table).await;

    let rec = record(&[
        ("name", AnyRedbType::new("Brand new".to_string())),
        ("price", AnyRedbType::new(7i64)),
    ]);
    table.replace_value("new", &rec).await.unwrap();

    let fetched = table
        .get_value("new")
        .await
        .unwrap()
        .expect("upserted row exists");
    assert_eq!(
        fetched["name"].try_get::<String>(),
        Some("Brand new".into())
    );
    assert_eq!(table.list_values().await.unwrap().len(), 3);
}

#[tokio::test]
async fn test_patch_merges_fields() {
    let (_tmp, table) = fresh_table("pat");
    seed(&table).await;

    let partial = record(&[("price", AnyRedbType::new(55i64))]);
    let returned = table.patch_value("a", &partial).await.unwrap();

    let fetched = table.get_value("a").await.unwrap().expect("row a exists");
    assert_eq!(fetched["price"].try_get::<i64>(), Some(55));
    assert_eq!(fetched["name"].try_get::<String>(), Some("Alpha".into()));
    // The result is the whole stored row, not only the change.
    assert_eq!(returned["price"].try_get::<i64>(), Some(55));
    assert_eq!(returned["name"].try_get::<String>(), Some("Alpha".into()));
}

#[tokio::test]
async fn test_patch_missing_row_errors() {
    let (_tmp, table) = fresh_table("patmiss");
    seed(&table).await;

    let partial = record(&[("price", AnyRedbType::new(55i64))]);
    let err = table
        .patch_value("nope", &partial)
        .await
        .expect_err("patching a missing row fails");

    assert!(err.is_not_found(), "{err:?}");
    assert!(table.get_value("nope").await.unwrap().is_none());
}

#[tokio::test]
async fn test_delete_one() {
    let (_tmp, table) = fresh_table("del");
    seed(&table).await;

    WritableValueSet::delete(&table, "a").await.unwrap();

    let all = table.list_values().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key("a"));
    assert!(all.contains_key("b"));
}

#[tokio::test]
async fn test_delete_of_a_missing_row_is_ok() {
    let (_tmp, table) = fresh_table("delmiss");
    seed(&table).await;

    WritableValueSet::delete(&table, "nope").await.unwrap();

    assert_eq!(table.list_values().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_delete_all() {
    let (_tmp, table) = fresh_table("delall");
    seed(&table).await;

    WritableValueSet::delete_all(&table).await.unwrap();
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_delete_all_can_be_repeated() {
    let (_tmp, table) = fresh_table("delall2");
    seed(&table).await;

    WritableValueSet::delete_all(&table).await.unwrap();
    WritableValueSet::delete_all(&table).await.unwrap();

    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_rows_written_after_delete_all_are_readable() {
    let (_tmp, table) = fresh_table("delallthenwrite");
    seed(&table).await;
    WritableValueSet::delete_all(&table).await.unwrap();

    seed(&table).await;

    assert_eq!(table.list_values().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_insert_return_id_generates_uuid() {
    let (_tmp, table) = fresh_table("auto");

    let rec = record(&[
        ("name", AnyRedbType::new("Auto".to_string())),
        ("price", AnyRedbType::new(42i64)),
    ]);
    let id = table.insert_return_id_value(&rec).await.unwrap();
    assert!(!id.is_empty());

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Auto".into()));
}

#[tokio::test]
async fn test_insert_return_id_makes_a_new_id_on_every_call() {
    let (_tmp, table) = fresh_table("autotwice");
    let rec = record(&[("name", AnyRedbType::new("Same".to_string()))]);

    let first = table.insert_return_id_value(&rec).await.unwrap();
    let second = table.insert_return_id_value(&rec).await.unwrap();

    assert_ne!(first, second);
    assert_eq!(table.list_values().await.unwrap().len(), 2);
}
