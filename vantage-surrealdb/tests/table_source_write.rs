use std::sync::atomic::{AtomicU32, Ordering};
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn get_db() -> SurrealDB {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dsn = format!("cbor://root:root@localhost:8000/bakery/write_test_{}", n);
    let client = surreal_client::SurrealConnection::dsn(&dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

fn make_table(db: SurrealDB, name: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("score")
}

fn make_record(name: &str, score: i64) -> Record<AnySurrealType> {
    let mut r = Record::new();
    r.insert("name".to_string(), AnySurrealType::new(name.to_string()));
    r.insert("score".to_string(), AnySurrealType::new(score));
    r
}

// -- insert_table_value --

#[tokio::test]
async fn test_insert_table_value() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_insert");

    db.execute(&surreal_expr!("DELETE tw_insert")).await.ok();

    let id = Thing::new("tw_insert", "a1");
    let record = make_record("Alice", 100);

    let returned = table
        .data_source()
        .insert_table_value(&table, &id, &record)
        .await
        .expect("insert_table_value failed");

    assert_eq!(
        returned.get("name").and_then(|v| v.try_get::<String>()),
        Some("Alice".to_string())
    );
    assert_eq!(
        returned.get("score").and_then(|v| v.try_get::<i64>()),
        Some(100)
    );

    // Verify via direct read
    let count = table.data_source().get_table_count(&table).await.unwrap();
    assert_eq!(count, 1);

    db.execute(&surreal_expr!("DELETE tw_insert")).await.ok();
}

// -- insert_table_return_id_value --

#[tokio::test]
async fn test_insert_table_return_id() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_retid");

    db.execute(&surreal_expr!("DELETE tw_retid")).await.ok();

    let record = make_record("Bob", 42);

    let id = table
        .data_source()
        .insert_table_return_id_value(&table, &record)
        .await
        .expect("insert_table_return_id_value failed");

    // ID should reference tw_retid table
    assert_eq!(id.to_string().split(':').next().unwrap(), "tw_retid");

    // Verify record exists
    let fetched = table
        .data_source()
        .get_table_value(&table, &id)
        .await
        .expect("get_table_value failed");

    assert_eq!(
        fetched.get("name").and_then(|v| v.try_get::<String>()),
        Some("Bob".to_string())
    );

    db.execute(&surreal_expr!("DELETE tw_retid")).await.ok();
}

// -- replace_table_value --

#[tokio::test]
async fn test_replace_table_value() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_replace");

    db.execute(&surreal_expr!("DELETE tw_replace")).await.ok();

    // Insert initial
    let id = Thing::new("tw_replace", "r1");
    let initial = make_record("Original", 10);
    table
        .data_source()
        .insert_table_value(&table, &id, &initial)
        .await
        .unwrap();

    // Replace with new content (CONTENT replaces all fields)
    let replacement = make_record("Replaced", 99);
    let returned = table
        .data_source()
        .replace_table_value(&table, &id, &replacement)
        .await
        .expect("replace_table_value failed");

    assert_eq!(
        returned.get("name").and_then(|v| v.try_get::<String>()),
        Some("Replaced".to_string())
    );
    assert_eq!(
        returned.get("score").and_then(|v| v.try_get::<i64>()),
        Some(99)
    );

    // Verify only 1 record
    let count = table.data_source().get_table_count(&table).await.unwrap();
    assert_eq!(count, 1);

    db.execute(&surreal_expr!("DELETE tw_replace")).await.ok();
}

// -- patch_table_value --

#[tokio::test]
async fn test_patch_table_value() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_patch");

    db.execute(&surreal_expr!("DELETE tw_patch")).await.ok();

    // Insert initial
    let id = Thing::new("tw_patch", "p1");
    let initial = make_record("PatchMe", 50);
    table
        .data_source()
        .insert_table_value(&table, &id, &initial)
        .await
        .unwrap();

    // Patch only the score (MERGE keeps other fields)
    let mut partial = Record::new();
    partial.insert("score".to_string(), AnySurrealType::new(75i64));

    let returned = table
        .data_source()
        .patch_table_value(&table, &id, &partial)
        .await
        .expect("patch_table_value failed");

    // Name should be preserved
    assert_eq!(
        returned.get("name").and_then(|v| v.try_get::<String>()),
        Some("PatchMe".to_string())
    );
    // Score should be updated
    assert_eq!(
        returned.get("score").and_then(|v| v.try_get::<i64>()),
        Some(75)
    );

    db.execute(&surreal_expr!("DELETE tw_patch")).await.ok();
}

// -- delete_table_value --

#[tokio::test]
async fn test_delete_table_value() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_del");

    db.execute(&surreal_expr!("DELETE tw_del")).await.ok();

    let id1 = Thing::new("tw_del", "d1");
    let id2 = Thing::new("tw_del", "d2");
    table
        .data_source()
        .insert_table_value(&table, &id1, &make_record("One", 1))
        .await
        .unwrap();
    table
        .data_source()
        .insert_table_value(&table, &id2, &make_record("Two", 2))
        .await
        .unwrap();

    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        2
    );

    table
        .data_source()
        .delete_table_value(&table, &id1)
        .await
        .expect("delete_table_value failed");

    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        1
    );

    // Deleted record should not be fetchable
    let remaining = table.data_source().list_table_values(&table).await.unwrap();
    assert!(remaining.contains_key(&id2));
    assert!(!remaining.contains_key(&id1));

    db.execute(&surreal_expr!("DELETE tw_del")).await.ok();
}

// -- delete_table_all_values --

#[tokio::test]
async fn test_delete_table_all_values() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_delall");

    db.execute(&surreal_expr!("DELETE tw_delall")).await.ok();

    for i in 0..5 {
        let id = Thing::new("tw_delall", format!("x{}", i));
        table
            .data_source()
            .insert_table_value(&table, &id, &make_record(&format!("Item{}", i), i))
            .await
            .unwrap();
    }

    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        5
    );

    table
        .data_source()
        .delete_table_all_values(&table)
        .await
        .expect("delete_table_all_values failed");

    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        0
    );
}

// -- round-trip: insert → read → patch → read → delete --

#[tokio::test]
async fn test_full_crud_lifecycle() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_crud");

    db.execute(&surreal_expr!("DELETE tw_crud")).await.ok();

    // Create
    let id = Thing::new("tw_crud", "lifecycle1");
    let record = make_record("Lifecycle", 0);
    table
        .data_source()
        .insert_table_value(&table, &id, &record)
        .await
        .unwrap();

    // Read
    let fetched = table
        .data_source()
        .get_table_value(&table, &id)
        .await
        .unwrap();
    assert_eq!(
        fetched.get("name").and_then(|v| v.try_get::<String>()),
        Some("Lifecycle".to_string())
    );

    // Update (patch)
    let mut patch = Record::new();
    patch.insert("score".to_string(), AnySurrealType::new(999i64));
    table
        .data_source()
        .patch_table_value(&table, &id, &patch)
        .await
        .unwrap();

    // Read again
    let updated = table
        .data_source()
        .get_table_value(&table, &id)
        .await
        .unwrap();
    assert_eq!(
        updated.get("score").and_then(|v| v.try_get::<i64>()),
        Some(999)
    );
    assert_eq!(
        updated.get("name").and_then(|v| v.try_get::<String>()),
        Some("Lifecycle".to_string())
    );

    // Replace
    let replacement = make_record("Replaced", 0);
    table
        .data_source()
        .replace_table_value(&table, &id, &replacement)
        .await
        .unwrap();

    let replaced = table
        .data_source()
        .get_table_value(&table, &id)
        .await
        .unwrap();
    assert_eq!(
        replaced.get("name").and_then(|v| v.try_get::<String>()),
        Some("Replaced".to_string())
    );

    // Delete
    table
        .data_source()
        .delete_table_value(&table, &id)
        .await
        .unwrap();

    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        0
    );

    db.execute(&surreal_expr!("DELETE tw_crud")).await.ok();
}

// -- insert with Thing reference field --

#[tokio::test]
async fn test_insert_with_thing_reference() {
    let db = get_db().await;
    let table = make_table(db.clone(), "tw_ref_child");

    db.execute(&surreal_expr!("DELETE tw_ref_parent"))
        .await
        .ok();
    db.execute(&surreal_expr!("DELETE tw_ref_child")).await.ok();

    // Create parent
    db.execute(&surreal_expr!(
        "CREATE tw_ref_parent:p1 SET name = {}",
        "Parent"
    ))
    .await
    .unwrap();

    // Insert child with Thing reference
    let child_id = Thing::new("tw_ref_child", "c1");
    let mut child_record = Record::new();
    child_record.insert("name".to_string(), AnySurrealType::new("Child".to_string()));
    child_record.insert("score".to_string(), AnySurrealType::new(10i64));
    child_record.insert(
        "parent".to_string(),
        AnySurrealType::new(Thing::new("tw_ref_parent", "p1")),
    );

    table
        .data_source()
        .insert_table_value(&table, &child_id, &child_record)
        .await
        .unwrap();

    // Verify relationship traversal
    let result = db
        .execute(&surreal_expr!(
            "SELECT VALUE parent.name FROM ONLY tw_ref_child:c1"
        ))
        .await
        .unwrap();

    assert_eq!(result.try_get::<String>(), Some("Parent".to_string()));

    db.execute(&surreal_expr!("DELETE tw_ref_parent"))
        .await
        .ok();
    db.execute(&surreal_expr!("DELETE tw_ref_child")).await.ok();
}
