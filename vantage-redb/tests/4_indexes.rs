//! Test 4: Atomic secondary index maintenance.
//!
//! These tests target the side-effect of write operations: every CRUD
//! call has to update the corresponding index tables inside the same
//! redb write transaction.

use vantage_dataset::prelude::*;
use vantage_redb::operation::RedbOperation;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

fn user_table() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("users", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column(Column::<String>::new("email").with_flag(ColumnFlag::Indexed))
        .with_column(Column::<String>::new("group").with_flag(ColumnFlag::Indexed));
    (path, table)
}

fn user(name: &str, email: &str, group: &str) -> Record<AnyRedbType> {
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("name".into(), AnyRedbType::new(name.to_string()));
    r.insert("email".into(), AnyRedbType::new(email.to_string()));
    r.insert("group".into(), AnyRedbType::new(group.to_string()));
    r
}

/// Ids of the rows an indexed lookup `column = value` finds.
async fn ids_where(table: &Table<Redb, EmptyEntity>, column: &str, value: &str) -> Vec<String> {
    let mut q = table.clone();
    q.add_condition(q[column].clone().eq(value));
    let mut ids: Vec<String> = q.list_values().await.unwrap().into_keys().collect();
    ids.sort();
    ids
}

// ── Insert path ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_creates_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    assert_eq!(ids_where(&table, "email", "alice@x.com").await, ["a"]);
}

#[tokio::test]
async fn test_insert_indexes_on_each_indexed_column() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    // Both flagged columns get an index.
    assert_eq!(ids_where(&table, "group", "admin").await, ["a"]);
    assert_eq!(ids_where(&table, "email", "alice@x.com").await, ["a"]);
}

// ── Non-unique columns ────────────────────────────────────────────────────

#[tokio::test]
async fn test_non_unique_column_returns_all_matches() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value("b", &user("Bob", "bob@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value("c", &user("Carol", "carol@x.com", "user"))
        .await
        .unwrap();

    assert_eq!(ids_where(&table, "group", "admin").await, ["a", "b"]);
    assert_eq!(ids_where(&table, "group", "user").await, ["c"]);
}

// ── Delete path ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_removes_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value("b", &user("Bob", "bob@x.com", "admin"))
        .await
        .unwrap();

    WritableValueSet::delete(&table, "a").await.unwrap();

    assert!(ids_where(&table, "email", "alice@x.com").await.is_empty());
    assert_eq!(ids_where(&table, "email", "bob@x.com").await, ["b"]);
    // The shared group value keeps pointing at the surviving row only.
    assert_eq!(ids_where(&table, "group", "admin").await, ["b"]);
}

// ── Replace path ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_replace_swaps_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    table
        .replace_value("a", &user("Alice Updated", "alice2@x.com", "user"))
        .await
        .unwrap();

    assert!(ids_where(&table, "email", "alice@x.com").await.is_empty());
    assert_eq!(ids_where(&table, "email", "alice2@x.com").await, ["a"]);
    assert!(ids_where(&table, "group", "admin").await.is_empty());
    assert_eq!(ids_where(&table, "group", "user").await, ["a"]);
}

// ── Patch path ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_patch_updates_only_changed_index() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    let mut partial: Record<AnyRedbType> = Record::new();
    partial.insert("email".into(), AnyRedbType::new("new@x.com".to_string()));
    table.patch_value("a", &partial).await.unwrap();

    assert!(ids_where(&table, "email", "alice@x.com").await.is_empty());
    assert_eq!(ids_where(&table, "email", "new@x.com").await, ["a"]);
    // The untouched column still finds the row.
    assert_eq!(ids_where(&table, "group", "admin").await, ["a"]);
}

// ── Delete-all path ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_all_drops_indexes() {
    let (_tmp, table) = user_table();
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        table
            .insert_value(
                id,
                &user(name, &format!("{}@x.com", name.to_lowercase()), "admin"),
            )
            .await
            .unwrap();
    }

    WritableValueSet::delete_all(&table).await.unwrap();

    assert!(ids_where(&table, "email", "alice@x.com").await.is_empty());
    assert!(ids_where(&table, "group", "admin").await.is_empty());
}

#[tokio::test]
async fn test_indexes_work_for_rows_written_after_delete_all() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    WritableValueSet::delete_all(&table).await.unwrap();

    table
        .insert_value("b", &user("Bob", "bob@x.com", "admin"))
        .await
        .unwrap();

    assert_eq!(ids_where(&table, "group", "admin").await, ["b"]);
}

#[tokio::test]
async fn test_conditional_delete_only_drops_matching_indexes() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value("b", &user("Bob", "bob@x.com", "user"))
        .await
        .unwrap();

    let mut conditional = table.clone();
    conditional.add_condition(conditional["group"].clone().eq("admin"));
    WritableValueSet::delete_all(&conditional).await.unwrap();

    assert_eq!(ids_where(&table, "email", "bob@x.com").await, ["b"]);
    assert!(ids_where(&table, "email", "alice@x.com").await.is_empty());
    assert!(ids_where(&table, "group", "admin").await.is_empty());
    assert_eq!(table.list_values().await.unwrap().len(), 1);
}

// ── Unflagged columns are not indexed ─────────────────────────────────────

#[tokio::test]
async fn test_unflagged_column_does_not_get_in_the_way_of_writes_and_lists() {
    let (_tmp, table) = user_table();
    table
        .insert_value("a", &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .replace_value("a", &user("Alicia", "alice@x.com", "admin"))
        .await
        .unwrap();

    // `name` is not flagged: conditioning on it first panics (see
    // 4_conditions.rs), but writing and listing never need an index for it.
    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows["a"]["name"].try_get::<String>(), Some("Alicia".into()));
}
