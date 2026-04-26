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
            &"a".to_string(),
            &record(&[
                ("name", AnyRedbType::new("Alpha".to_string())),
                ("price", AnyRedbType::new(10i64)),
            ]),
        )
        .await
        .unwrap();
    table
        .insert_value(
            &"b".to_string(),
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
    let result = table.insert_value(&"c".to_string(), &rec).await.unwrap();
    assert_eq!(result["name"].try_get::<String>(), Some("Gamma".into()));
    assert_eq!(result["price"].try_get::<i64>(), Some(30));

    let fetched = table
        .get_value(&"c".to_string())
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Gamma".into()));
}

#[tokio::test]
async fn test_replace_overwrites_all_fields() {
    let (_tmp, table) = fresh_table("rep");
    seed(&table).await;

    let rec = record(&[
        ("name", AnyRedbType::new("Alpha Replaced".to_string())),
        ("price", AnyRedbType::new(99i64)),
    ]);
    table.replace_value(&"a".to_string(), &rec).await.unwrap();

    let fetched = table
        .get_value(&"a".to_string())
        .await
        .unwrap()
        .expect("row a exists");
    assert_eq!(
        fetched["name"].try_get::<String>(),
        Some("Alpha Replaced".into())
    );
    assert_eq!(fetched["price"].try_get::<i64>(), Some(99));
}

#[tokio::test]
async fn test_patch_merges_fields() {
    let (_tmp, table) = fresh_table("pat");
    seed(&table).await;

    let partial = record(&[("price", AnyRedbType::new(55i64))]);
    table.patch_value(&"a".to_string(), &partial).await.unwrap();

    let fetched = table
        .get_value(&"a".to_string())
        .await
        .unwrap()
        .expect("row a exists");
    assert_eq!(fetched["price"].try_get::<i64>(), Some(55));
    // name untouched
    assert_eq!(fetched["name"].try_get::<String>(), Some("Alpha".into()));
}

#[tokio::test]
async fn test_patch_missing_row_errors() {
    let (_tmp, table) = fresh_table("patmiss");
    seed(&table).await;

    let partial = record(&[("price", AnyRedbType::new(55i64))]);
    let result = table.patch_value(&"nope".to_string(), &partial).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_one() {
    let (_tmp, table) = fresh_table("del");
    seed(&table).await;

    WritableValueSet::delete(&table, &"a".to_string())
        .await
        .unwrap();

    let all = table.list_values().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key("a"));
    assert!(all.contains_key("b"));
}

#[tokio::test]
async fn test_delete_all() {
    let (_tmp, table) = fresh_table("delall");
    seed(&table).await;

    WritableValueSet::delete_all(&table).await.unwrap();
    assert!(table.list_values().await.unwrap().is_empty());
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
async fn test_replace_creates_if_missing() {
    let (_tmp, table) = fresh_table("repmiss");
    seed(&table).await;

    // redb's insert overwrites by default; replace_value should also work
    // for a missing id (acts as upsert).
    let rec = record(&[
        ("name", AnyRedbType::new("Brand new".to_string())),
        ("price", AnyRedbType::new(7i64)),
    ]);
    table.replace_value(&"new".to_string(), &rec).await.unwrap();

    let fetched = table
        .get_value(&"new".to_string())
        .await
        .unwrap()
        .expect("upserted row exists");
    assert_eq!(
        fetched["name"].try_get::<String>(),
        Some("Brand new".into())
    );
}
