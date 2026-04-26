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

// ── Insert path ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_creates_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("a"));
}

#[tokio::test]
async fn test_insert_indexes_on_each_indexed_column() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    // Lookup by `group` should also work — both flagged columns get indexes.
    let mut q = table.clone();
    q.add_condition(q["group"].clone().eq("admin"));
    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("a"));
}

// ── Non-unique columns ────────────────────────────────────────────────────

#[tokio::test]
async fn test_non_unique_column_returns_all_matches() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value(&"b".to_string(), &user("Bob", "bob@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value(&"c".to_string(), &user("Carol", "carol@x.com", "user"))
        .await
        .unwrap();

    let mut q = table.clone();
    q.add_condition(q["group"].clone().eq("admin"));
    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a"));
    assert!(rows.contains_key("b"));
}

// ── Delete path ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_removes_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value(&"b".to_string(), &user("Bob", "bob@x.com", "admin"))
        .await
        .unwrap();

    WritableValueSet::delete(&table, &"a".to_string())
        .await
        .unwrap();

    // Alice's email no longer indexed.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    assert!(q.list_values().await.unwrap().is_empty());

    // Bob still indexed.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("bob@x.com"));
    assert_eq!(q.list_values().await.unwrap().len(), 1);
}

// ── Replace path ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_replace_swaps_index_entries() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    // Replace with new email and group.
    table
        .replace_value(
            &"a".to_string(),
            &user("Alice Updated", "alice2@x.com", "user"),
        )
        .await
        .unwrap();

    // Old email no longer indexed.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    assert!(q.list_values().await.unwrap().is_empty());

    // New email is indexed.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice2@x.com"));
    assert_eq!(q.list_values().await.unwrap().len(), 1);

    // Old group no longer indexed for this row.
    let mut q = table.clone();
    q.add_condition(q["group"].clone().eq("admin"));
    assert!(q.list_values().await.unwrap().is_empty());
}

// ── Patch path ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_patch_updates_only_changed_index() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    // Patch only the email — group should stay indexed under "admin".
    let mut partial: Record<AnyRedbType> = Record::new();
    partial.insert("email".into(), AnyRedbType::new("new@x.com".to_string()));
    table.patch_value(&"a".to_string(), &partial).await.unwrap();

    // Old email gone.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    assert!(q.list_values().await.unwrap().is_empty());

    // New email present.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("new@x.com"));
    assert_eq!(q.list_values().await.unwrap().len(), 1);

    // Group lookup still works.
    let mut q = table.clone();
    q.add_condition(q["group"].clone().eq("admin"));
    assert_eq!(q.list_values().await.unwrap().len(), 1);
}

// ── Delete-all path ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_all_drops_indexes() {
    let (_tmp, table) = user_table();
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        table
            .insert_value(
                &id.to_string(),
                &user(name, &format!("{}@x.com", name.to_lowercase()), "admin"),
            )
            .await
            .unwrap();
    }

    WritableValueSet::delete_all(&table).await.unwrap();

    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    assert!(q.list_values().await.unwrap().is_empty());

    let mut q = table.clone();
    q.add_condition(q["group"].clone().eq("admin"));
    assert!(q.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_conditional_delete_only_drops_matching_indexes() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();
    table
        .insert_value(&"b".to_string(), &user("Bob", "bob@x.com", "user"))
        .await
        .unwrap();

    // Conditional delete: only group=admin rows.
    let mut conditional = table.clone();
    conditional.add_condition(conditional["group"].clone().eq("admin"));
    WritableValueSet::delete_all(&conditional).await.unwrap();

    // Bob still present.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("bob@x.com"));
    assert_eq!(q.list_values().await.unwrap().len(), 1);

    // Alice's index entries cleared.
    let mut q = table.clone();
    q.add_condition(q["email"].clone().eq("alice@x.com"));
    assert!(q.list_values().await.unwrap().is_empty());
}

// ── Unflagged columns are not indexed ─────────────────────────────────────

#[tokio::test]
async fn test_unflagged_column_has_no_index_table() {
    let (_tmp, table) = user_table();
    table
        .insert_value(&"a".to_string(), &user("Alice", "alice@x.com", "admin"))
        .await
        .unwrap();

    // `name` was not flagged. The fact that conditioning on it panics is
    // covered in 4_conditions.rs; here we just verify that listing works
    // (i.e. the missing index doesn't trip up insert/list paths).
    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
}
