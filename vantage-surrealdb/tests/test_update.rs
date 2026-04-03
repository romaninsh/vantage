use std::sync::atomic::{AtomicU32, Ordering};
use vantage_expressions::ExprDataSource;
use vantage_expressions::Expressive;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_surrealdb::update::SurrealUpdate;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn get_db() -> SurrealDB {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dsn = format!("cbor://root:root@localhost:8000/bakery/update_test_{}", n);
    let client = surreal_client::SurrealConnection::dsn(&dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

async fn seed(db: &SurrealDB, table: &str, id: &str, name: &str, score: i64) {
    use vantage_surrealdb::insert::SurrealInsert;
    let insert = SurrealInsert::new(table)
        .with_id(id)
        .set_field("name", name.to_string())
        .set_field("score", score);
    db.execute(&insert.expr()).await.expect("seed failed");
}

async fn read_field<T: vantage_surrealdb::types::SurrealType>(
    db: &SurrealDB,
    record: &str,
    field: &str,
) -> T {
    let result = db
        .execute(&surreal_expr!(&format!(
            "SELECT VALUE {} FROM ONLY {}",
            field, record
        )))
        .await
        .expect("read_field query failed");
    result.try_get::<T>().expect("read_field type conversion")
}

// -- SET mode --

#[tokio::test]
async fn test_update_set_single_field() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_set1")).await.ok();
    seed(&db, "tu_set1", "a", "Alice", 10).await;

    let update = SurrealUpdate::new(Thing::new("tu_set1", "a")).set_field("score", 99i64);

    db.execute(&update.expr()).await.expect("update failed");

    // score updated
    assert_eq!(read_field::<i64>(&db, "tu_set1:a", "score").await, 99);
    // name preserved
    assert_eq!(
        read_field::<String>(&db, "tu_set1:a", "name").await,
        "Alice"
    );

    db.execute(&surreal_expr!("DELETE tu_set1")).await.ok();
}

#[tokio::test]
async fn test_update_set_multiple_fields() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_set2")).await.ok();
    seed(&db, "tu_set2", "b", "Bob", 20).await;

    let update = SurrealUpdate::new(Thing::new("tu_set2", "b"))
        .set_field("name", "Bobby".to_string())
        .set_field("score", 200i64);

    db.execute(&update.expr()).await.expect("update failed");

    assert_eq!(
        read_field::<String>(&db, "tu_set2:b", "name").await,
        "Bobby"
    );
    assert_eq!(read_field::<i64>(&db, "tu_set2:b", "score").await, 200);

    db.execute(&surreal_expr!("DELETE tu_set2")).await.ok();
}

// -- CONTENT mode (replaces all fields) --

#[tokio::test]
async fn test_update_content_replaces_all() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_content")).await.ok();
    seed(&db, "tu_content", "c", "Charlie", 30).await;

    let update = SurrealUpdate::new(Thing::new("tu_content", "c"))
        .content()
        .set_field("label", "Replaced".to_string());

    db.execute(&update.expr()).await.expect("update failed");

    // new field exists
    assert_eq!(
        read_field::<String>(&db, "tu_content:c", "label").await,
        "Replaced"
    );

    // old fields should be gone — query returns NONE
    let result = db
        .execute(&surreal_expr!("SELECT VALUE name FROM ONLY tu_content:c"))
        .await
        .unwrap();
    assert!(
        result.try_get::<String>().is_none(),
        "old 'name' field should be gone after CONTENT"
    );

    db.execute(&surreal_expr!("DELETE tu_content")).await.ok();
}

// -- MERGE mode (partial update, keeps unmentioned) --

#[tokio::test]
async fn test_update_merge_partial() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_merge")).await.ok();
    seed(&db, "tu_merge", "d", "Diana", 40).await;

    let update = SurrealUpdate::new(Thing::new("tu_merge", "d"))
        .merge()
        .set_field("score", 400i64);

    db.execute(&update.expr()).await.expect("update failed");

    // score updated
    assert_eq!(read_field::<i64>(&db, "tu_merge:d", "score").await, 400);
    // name preserved
    assert_eq!(
        read_field::<String>(&db, "tu_merge:d", "name").await,
        "Diana"
    );

    db.execute(&surreal_expr!("DELETE tu_merge")).await.ok();
}

#[tokio::test]
async fn test_update_merge_adds_new_field() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_merge2")).await.ok();
    seed(&db, "tu_merge2", "e", "Eve", 50).await;

    let update = SurrealUpdate::new(Thing::new("tu_merge2", "e"))
        .merge()
        .set_field("email", "eve@example.com".to_string());

    db.execute(&update.expr()).await.expect("update failed");

    assert_eq!(
        read_field::<String>(&db, "tu_merge2:e", "email").await,
        "eve@example.com"
    );
    assert_eq!(
        read_field::<String>(&db, "tu_merge2:e", "name").await,
        "Eve"
    );
    assert_eq!(read_field::<i64>(&db, "tu_merge2:e", "score").await, 50);

    db.execute(&surreal_expr!("DELETE tu_merge2")).await.ok();
}

// -- set_record convenience --

#[tokio::test]
async fn test_update_set_record() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_rec")).await.ok();
    seed(&db, "tu_rec", "f", "Frank", 60).await;

    let mut record = vantage_types::Record::new();
    record.insert(
        "name".to_string(),
        AnySurrealType::new("Franco".to_string()),
    );
    record.insert("score".to_string(), AnySurrealType::new(600i64));

    let update = SurrealUpdate::new(Thing::new("tu_rec", "f")).set_record(&record);

    db.execute(&update.expr()).await.expect("update failed");

    assert_eq!(
        read_field::<String>(&db, "tu_rec:f", "name").await,
        "Franco"
    );
    assert_eq!(read_field::<i64>(&db, "tu_rec:f", "score").await, 600);

    db.execute(&surreal_expr!("DELETE tu_rec")).await.ok();
}

// -- Thing reference field --

#[tokio::test]
async fn test_update_with_thing_reference() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_ref_owner")).await.ok();
    db.execute(&surreal_expr!("DELETE tu_ref_item")).await.ok();

    db.execute(&surreal_expr!(
        "CREATE tu_ref_owner:o1 SET name = {}",
        "Owner1"
    ))
    .await
    .unwrap();
    seed(&db, "tu_ref_item", "i1", "Item1", 0).await;

    let update = SurrealUpdate::new(Thing::new("tu_ref_item", "i1"))
        .set_field("owner", Thing::new("tu_ref_owner", "o1"));

    db.execute(&update.expr()).await.expect("update failed");

    // Verify traversal
    assert_eq!(
        read_field::<String>(&db, "tu_ref_item:i1", "owner.name").await,
        "Owner1"
    );

    db.execute(&surreal_expr!("DELETE tu_ref_owner")).await.ok();
    db.execute(&surreal_expr!("DELETE tu_ref_item")).await.ok();
}

// -- bool / float types --

#[tokio::test]
async fn test_update_various_types() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE tu_types")).await.ok();
    seed(&db, "tu_types", "t1", "Typed", 0).await;

    let update = SurrealUpdate::new(Thing::new("tu_types", "t1"))
        .set_field("active", true)
        .set_field("ratio", 3.14f64);

    db.execute(&update.expr()).await.expect("update failed");

    assert!(read_field::<bool>(&db, "tu_types:t1", "active").await);
    assert_eq!(read_field::<f64>(&db, "tu_types:t1", "ratio").await, 3.14);
    // original fields still there
    assert_eq!(
        read_field::<String>(&db, "tu_types:t1", "name").await,
        "Typed"
    );

    db.execute(&surreal_expr!("DELETE tu_types")).await.ok();
}
