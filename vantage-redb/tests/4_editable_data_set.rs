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
async fn test_delete_removes_only_the_named_row_and_a_missing_id_is_ok() {
    let (_tmp, table) = fresh_table("del");
    seed(&table).await;

    WritableValueSet::delete(&table, "a").await.unwrap();
    WritableValueSet::delete(&table, "nope").await.unwrap();

    let all = table.list_values().await.unwrap();
    assert_eq!(all.keys().collect::<Vec<_>>(), ["b"]);
}

#[tokio::test]
async fn test_delete_all_can_be_repeated_and_the_table_takes_new_rows() {
    let (_tmp, table) = fresh_table("delall");
    seed(&table).await;

    WritableValueSet::delete_all(&table).await.unwrap();
    WritableValueSet::delete_all(&table).await.unwrap();
    assert!(table.list_values().await.unwrap().is_empty());

    seed(&table).await;
    assert_eq!(table.list_values().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_insert_return_id_makes_a_new_id_unless_one_is_supplied() {
    let (_tmp, table) = fresh_table("auto");
    let rec = record(&[("name", AnyRedbType::new("Auto".to_string()))]);
    let given = record(&[
        ("id", AnyRedbType::new("given".to_string())),
        ("name", AnyRedbType::new("Given".to_string())),
    ]);

    let first = table.insert_return_id_value(&rec).await.unwrap();
    let second = table.insert_return_id_value(&rec).await.unwrap();
    let supplied = table.insert_return_id_value(&given).await.unwrap();

    assert!(!first.is_empty());
    assert_ne!(first, second);
    assert_eq!(supplied, "given");
    let fetched = table.get_value(&first).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Auto".into()));
    assert!(table.get_value("given").await.unwrap().is_some());
    assert_eq!(table.list_values().await.unwrap().len(), 3);
}
