use serde_json::{Value, json};

use vantage_dataset::{
    im::{ImDataSource, ImTable},
    traits::{InsertableValueSet, ReadableValueSet, WritableValueSet},
};
use vantage_types::Record;

fn table(ds: &ImDataSource, name: &str) -> ImTable<(), Value> {
    ImTable::new(ds, name)
}

fn rec(value: Value) -> Record<Value> {
    Record::from(value)
}

#[tokio::test]
async fn empty_table_reads_nothing() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    assert!(t.list_values().await.unwrap().is_empty());
    assert!(t.get_value("nope").await.unwrap().is_none());
    assert!(t.get_some_value().await.unwrap().is_none());
}

#[tokio::test]
async fn list_preserves_insertion_order_and_get_some_returns_first() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");
    for id in ["c", "a", "b"] {
        t.insert_value(id, &rec(json!({"name": id}))).await.unwrap();
    }

    let ids: Vec<String> = t.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["c", "a", "b"]);

    let (id, row) = t.get_some_value().await.unwrap().expect("a row exists");
    assert_eq!(id, "c");
    assert_eq!(row["name"], json!("c"));

    let got = t.get_value("a").await.unwrap().expect("a exists");
    assert_eq!(got["name"], json!("a"));
}

#[tokio::test]
async fn insert_return_id_value_generates_an_id_when_none_is_given() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let first = t
        .insert_return_id_value(&rec(json!({"name": "Alice"})))
        .await
        .unwrap();
    let second = t
        .insert_return_id_value(&rec(json!({"name": "Alice"})))
        .await
        .unwrap();

    assert!(!first.is_empty());
    assert_ne!(first, second, "every call creates a new record");
    assert_eq!(t.list_values().await.unwrap().len(), 2);
    let stored = t.get_value(first).await.unwrap().expect("stored");
    assert_eq!(stored["name"], json!("Alice"));
}

#[tokio::test]
async fn insert_return_id_value_uses_the_id_in_the_record() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let id = t
        .insert_return_id_value(&rec(json!({"id": "user-123", "name": "Bob"})))
        .await
        .unwrap();

    assert_eq!(id, "user-123");
    let stored = t.get_value("user-123").await.unwrap().expect("stored");
    assert_eq!(stored["name"], json!("Bob"));
}

#[tokio::test]
async fn insert_return_id_value_generates_an_id_for_a_null_id() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let id = t
        .insert_return_id_value(&rec(json!({"id": null, "name": "Charlie"})))
        .await
        .unwrap();

    assert!(!id.is_empty());
    assert_ne!(id, "null");
    assert!(t.get_value(id).await.unwrap().is_some());
}

#[tokio::test]
async fn insert_value_is_idempotent() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let stored = t
        .insert_value("k", &rec(json!({"name": "first"})))
        .await
        .unwrap();
    assert_eq!(stored["name"], json!("first"));

    let again = t
        .insert_value("k", &rec(json!({"name": "second"})))
        .await
        .unwrap();
    assert_eq!(again["name"], json!("first"), "returns the stored row");
    let row = t.get_value("k").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("first"), "never overwrites");
}

#[tokio::test]
async fn replace_value_creates_then_replaces_the_whole_record() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let created = t
        .replace_value("k", &rec(json!({"name": "a", "age": 1})))
        .await
        .unwrap();
    assert_eq!(created["name"], json!("a"));
    assert!(t.get_value("k").await.unwrap().is_some());

    let replaced = t
        .replace_value("k", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(replaced["name"], json!("b"));
    let row = t.get_value("k").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("b"));
    assert!(row.get("age").is_none(), "attributes not sent are removed");
}

#[tokio::test]
async fn patch_value_merges_and_returns_the_whole_row() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");
    t.insert_value("k", &rec(json!({"name": "a", "age": 1})))
        .await
        .unwrap();

    let patched = t
        .patch_value("k", &rec(json!({"name": "b"})))
        .await
        .unwrap();

    assert_eq!(patched["name"], json!("b"));
    assert_eq!(patched["age"], json!(1), "the rest of the row is kept");
    let row = t.get_value("k").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("b"));
    assert_eq!(row["age"], json!(1));
}

#[tokio::test]
async fn patch_value_of_a_missing_row_is_not_found() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    let err = t
        .patch_value("nope", &rec(json!({"name": "x"})))
        .await
        .unwrap_err();

    assert!(err.is_not_found(), "{err}");
    assert!(t.get_value("nope").await.unwrap().is_none());
}

#[tokio::test]
async fn delete_is_idempotent_and_delete_all_empties_the_table() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");
    t.delete("nope").await.unwrap();

    t.insert_value("a", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    t.insert_value("b", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    t.delete("a").await.unwrap();
    t.delete("a").await.unwrap();
    assert!(t.get_value("a").await.unwrap().is_none());
    assert!(t.get_value("b").await.unwrap().is_some());

    t.delete_all().await.unwrap();
    t.delete_all().await.unwrap();
    assert!(t.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn tables_in_one_data_source_are_isolated() {
    let ds = ImDataSource::new();
    let users = table(&ds, "users");
    let products = table(&ds, "products");
    users
        .insert_value("u", &rec(json!({"name": "u"})))
        .await
        .unwrap();
    products
        .insert_value("p", &rec(json!({"name": "p"})))
        .await
        .unwrap();

    users.delete_all().await.unwrap();

    assert!(users.list_values().await.unwrap().is_empty());
    assert_eq!(products.list_values().await.unwrap().len(), 1);
}

#[cfg(feature = "contract")]
#[tokio::test]
async fn im_table_meets_the_unnarrowed_contract() {
    let ds = ImDataSource::new();
    let t = table(&ds, "users");

    vantage_dataset::contract::check_unnarrowed(&t, |s| s.to_string(), |s| json!(s)).await;
}
