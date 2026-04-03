use std::sync::atomic::{AtomicU32, Ordering};
use vantage_expressions::ExprDataSource;
use vantage_expressions::Expressive;
use vantage_surrealdb::insert::SurrealInsert;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn get_db() -> SurrealDB {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dsn = format!("cbor://root:root@localhost:8000/bakery/insert_test_{}", n);
    let client = surreal_client::SurrealConnection::dsn(&dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

#[tokio::test]
async fn test_insert_and_read_back() {
    let db = get_db().await;

    // Clean up
    db.execute(&surreal_expr!("DELETE test_product")).await.ok();

    let insert = SurrealInsert::new("test_product")
        .with_id("sandwich")
        .set_field("name", "Nuclear Sandwich".to_string())
        .set_field("calories", 9001i64)
        .set_field("price", 12.5f64);

    db.execute(&insert.expr()).await.expect("insert failed");

    // Read back
    let result = db
        .execute(&surreal_expr!("SELECT * FROM ONLY test_product:sandwich"))
        .await
        .expect("select failed");

    let map: indexmap::IndexMap<String, AnySurrealType> = result.try_get().expect("parse failed");
    assert_eq!(
        map.get("name").and_then(|v| v.try_get::<String>()),
        Some("Nuclear Sandwich".to_string())
    );
    assert_eq!(
        map.get("calories").and_then(|v| v.try_get::<i64>()),
        Some(9001)
    );
    assert_eq!(
        map.get("price").and_then(|v| v.try_get::<f64>()),
        Some(12.5)
    );

    // Cleanup
    db.execute(&surreal_expr!("DELETE test_product")).await.ok();
}

#[tokio::test]
async fn test_insert_without_id() {
    let db = get_db().await;

    db.execute(&surreal_expr!("DELETE test_auto")).await.ok();

    let insert = SurrealInsert::new("test_auto").set_field("label", "auto-id".to_string());

    db.execute(&insert.expr()).await.expect("insert failed");

    let count = db
        .execute(&surreal_expr!(
            "RETURN count(SELECT VALUE id FROM test_auto)"
        ))
        .await
        .expect("count failed");

    assert_eq!(count.try_get::<i64>(), Some(1));

    db.execute(&surreal_expr!("DELETE test_auto")).await.ok();
}

#[tokio::test]
async fn test_insert_with_thing_field() {
    let db = get_db().await;

    db.execute(&surreal_expr!("DELETE test_owner")).await.ok();
    db.execute(&surreal_expr!("DELETE test_pet")).await.ok();

    // Create owner
    db.execute(&surreal_expr!(
        "CREATE test_owner:alice SET name = {}",
        "Alice"
    ))
    .await
    .expect("create owner failed");

    // Insert pet with Thing reference to owner
    let insert = SurrealInsert::new("test_pet")
        .with_id("fido")
        .set_field("name", "Fido".to_string())
        .set_field("owner", Thing::new("test_owner", "alice"));

    db.execute(&insert.expr()).await.expect("insert pet failed");

    // Verify relationship traversal
    let result = db
        .execute(&surreal_expr!(
            "SELECT VALUE owner.name FROM ONLY test_pet:fido"
        ))
        .await
        .expect("traversal failed");

    assert_eq!(result.try_get::<String>(), Some("Alice".to_string()));

    db.execute(&surreal_expr!("DELETE test_owner")).await.ok();
    db.execute(&surreal_expr!("DELETE test_pet")).await.ok();
}

#[tokio::test]
async fn test_insert_multiple_types() {
    let db = get_db().await;

    db.execute(&surreal_expr!("DELETE test_types")).await.ok();

    let insert = SurrealInsert::new("test_types")
        .with_id("t1")
        .set_field("flag", true)
        .set_field("count", 42i64)
        .set_field("ratio", 3.14f64)
        .set_field("label", "hello".to_string());

    db.execute(&insert.expr()).await.expect("insert failed");

    let result = db
        .execute(&surreal_expr!("SELECT * FROM ONLY test_types:t1"))
        .await
        .expect("select failed");

    let map: indexmap::IndexMap<String, AnySurrealType> = result.try_get().expect("parse failed");
    assert_eq!(
        map.get("flag").and_then(|v| v.try_get::<bool>()),
        Some(true)
    );
    assert_eq!(map.get("count").and_then(|v| v.try_get::<i64>()), Some(42));
    assert_eq!(
        map.get("ratio").and_then(|v| v.try_get::<f64>()),
        Some(3.14)
    );
    assert_eq!(
        map.get("label").and_then(|v| v.try_get::<String>()),
        Some("hello".to_string())
    );

    db.execute(&surreal_expr!("DELETE test_types")).await.ok();
}
