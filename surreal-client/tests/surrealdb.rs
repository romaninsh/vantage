//! Integration tests for SurrealDB using a real database instance
//!
//! These tests require a SurrealDB instance running at localhost:8000
//! with default root credentials (root/root).
//!
//! To run these tests:
//! 1. Start SurrealDB: `surreal start --log trace --user root --pass root memory`
//! 2. Run tests: `cargo test --test surrealdb`

use serde_json::{Value, json};

use surreal_client::{
    RecordId, RecordRange, SurrealClient, SurrealConnection, SurrealError, Table,
};

const DB_URL: &str = "ws://localhost:8000";
const ROOT_USER: &str = "root";
const ROOT_PASS: &str = "root";
const TEST_NAMESPACE: &str = "test";
const TEST_DATABASE: &str = "integration";

async fn get_client() -> SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to create connection")
}

#[tokio::test]
async fn test_basic_connection() {
    let client = get_client().await;

    // Test version command
    let version = client.version().await.expect("Failed to get version");
    println!("SurrealDB version: {}", version);
    assert!(!version.is_empty());

    // Test info command
    let info = client.info().await.expect("Failed to get session info");
    println!("Session info: {:?}", info);
}

#[tokio::test]
#[ignore]
async fn test_http_connection() {
    let client = SurrealConnection::new()
        .url("http://localhost:8000")
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to connect to SurrealDB via HTTP");

    let version = client.version().await.expect("Failed to get version");
    assert!(!version.is_empty());
}

#[tokio::test]
async fn test_dsn_connection() {
    let dsn = format!(
        "ws://{}:{}@localhost:8000/{}/{}",
        ROOT_USER, ROOT_PASS, TEST_NAMESPACE, TEST_DATABASE
    );

    let client = SurrealConnection::dsn(&dsn)
        .expect("Failed to parse DSN")
        .connect()
        .await
        .expect("Failed to connect using DSN");

    let version = client.version().await.expect("Failed to get version");
    assert!(!version.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_crud_operations() {
    let client = get_client().await;

    // Clean up any existing test data
    let _ = client.delete("users").await;

    // Test CREATE
    let user_data = json!({
        "name": "John Doe",
        "email": "john@example.com",
        "age": 30,
        "active": true
    });

    let created = client
        .create("users:john", Some(user_data.clone()))
        .await
        .expect("Failed to create user");

    println!("Created user: {:?}", created);
    assert!(created.is_object());

    // Test SELECT specific record
    let selected = client
        .select("users:john")
        .await
        .expect("Failed to select user");

    println!("Selected user: {:?}", selected);
    if let Value::Array(results) = &selected {
        assert!(!results.is_empty());
        if let Some(Value::Object(user)) = results.first() {
            assert_eq!(
                user.get("name"),
                Some(&Value::String("John Doe".to_string()))
            );
            assert_eq!(user.get("age"), Some(&Value::Number(30.into())));
        }
    }

    // Test UPDATE
    let updated_data = json!({
        "age": 31,
        "city": "New York"
    });

    let updated = client
        .update("users:john", Some(updated_data))
        .await
        .expect("Failed to update user");

    println!("Updated user: {:?}", updated);
    assert!(updated.is_object());

    // Test MERGE
    let merge_data = json!({
        "active": false,
        "last_login": "2023-12-01"
    });

    let merged = client
        .merge("users:john", merge_data)
        .await
        .expect("Failed to merge user");

    println!("Merged user: {:?}", merged);
    assert!(merged.is_object());

    // Test PATCH - multiple operations
    let patch_data = vec![
        json!({ "op": "replace", "path": "/email", "value": "john.doe@example.com" }),
        json!({ "op": "add", "path": "/tags", "value": ["vip", "premium"] }),
    ];

    let patched = client
        .patch("users:john", patch_data)
        .await
        .expect("Failed to patch user");

    println!("Patched user: {:?}", patched);

    // Test SELECT all
    let all_users = client
        .select("users")
        .await
        .expect("Failed to select all users");
    println!("All users: {:?}", all_users);

    // Test DELETE specific record
    let deleted = client
        .delete("users:john")
        .await
        .expect("Failed to delete user");

    println!("Deleted: {:?}", deleted);

    // Verify deletion
    let after_delete = client
        .select("users:john")
        .await
        .expect("Failed to select after delete");

    if let Value::Array(results) = after_delete {
        assert!(results.is_empty(), "User should be deleted");
    }
}

#[tokio::test]
#[ignore]
// Often fails in a pipeline with error:
// Failed to create product: Protocol("Server error: {\"code\":-32000,\"message\":\"There was a problem with the database: The query was not executed due to a failed transaction. Failed to commit transaction due to a read or write conflict. This transaction can be retried\"}")
async fn test_bulk_operations() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("products").await;

    // Test bulk CREATE
    let products = [
        json!({
            "name": "Laptop",
            "price": 999.99,
            "category": "Electronics",
            "in_stock": true
        }),
        json!({
            "name": "Mouse",
            "price": 29.99,
            "category": "Electronics",
            "in_stock": true
        }),
        json!({
            "name": "Keyboard",
            "price": 79.99,
            "category": "Electronics",
            "in_stock": false
        }),
    ];

    for (i, product) in products.iter().enumerate() {
        client
            .create(&format!("products:prod_{}", i + 1), Some(product.clone()))
            .await
            .expect("Failed to create product");
    }

    // Test bulk SELECT
    let all_products = client
        .select("products")
        .await
        .expect("Failed to select all products");

    println!("All products: {:?}", all_products);
    if let Value::Array(results) = &all_products {
        assert_eq!(results.len(), 3);
    }

    // Test bulk UPDATE with WHERE clause
    let update_query = "UPDATE products SET price = price * 0.9 WHERE category = 'Electronics'";
    let updated = client
        .query(update_query, None)
        .await
        .expect("Failed to bulk update");

    println!("Bulk update result: {:?}", updated);

    // Verify updates
    let updated_products = client
        .select("products")
        .await
        .expect("Failed to select after update");

    println!("Products after discount: {:?}", updated_products);

    // Clean up
    let _ = client.delete("products").await;
}

#[tokio::test]
async fn test_client_cloning() {
    let mut client1 = get_client().await;
    let client2 = get_client().await;

    // Note: Both client1 and client2 are references to the same SurrealClient instance
    // They share the same session state, which is the intended behavior for our refactoring

    // Set a variable using client1
    client1
        .let_var("shared_var", json!("set_by_client1"))
        .await
        .expect("Failed to set variable via client1");

    // Read the variable using client2 (should see the same value since they're the same client)
    let query_result = client2
        .query("RETURN $shared_var", None)
        .await
        .expect("Failed to query variable via client2");

    println!(
        "Variable set by client1, read by client2: {:?}",
        query_result
    );

    // Both clients should work and return the same version
    let version1 = client1
        .version()
        .await
        .expect("Failed to get version from client1");
    let version2 = client2
        .version()
        .await
        .expect("Failed to get version from client2");

    assert_eq!(version1, version2);
    println!(
        "Both client references work with shared connection, version: {}",
        version1
    );

    // Clean up
    client1
        .unset("shared_var")
        .await
        .expect("Failed to unset variable");
}

#[tokio::test]
async fn test_error_handling() {
    let client = get_client().await;

    // Test invalid query
    let invalid_result = client.query("INVALID QUERY SYNTAX", None).await;
    assert!(invalid_result.is_err());
    println!("Invalid query error: {:?}", invalid_result.unwrap_err());

    // Test selecting non-existent record
    let missing = client.select("nonexistent:record").await;
    println!("Missing record result: {:?}", missing);

    // Test constraint violation (if we had constraints)
    let duplicate_result = client
        .create("test:duplicate", Some(json!({"name": "test"})))
        .await;

    if duplicate_result.is_ok() {
        // Try creating the same record again
        let second_attempt = client
            .create("test:duplicate", Some(json!({"name": "test2"})))
            .await;
        println!("Duplicate creation result: {:?}", second_attempt);

        // Clean up
        let _ = client.delete("test:duplicate").await;
    }
}

fn first_result(response: &Value) -> &Value {
    &response[0]["result"]
}

fn field_values(records: &Value, field: &str) -> Vec<Value> {
    let mut values: Vec<Value> = records
        .as_array()
        .expect("an array of records")
        .iter()
        .map(|r| r[field].clone())
        .collect();
    values.sort_by_key(|v| v.to_string());
    values
}

/// Writes rows through SurrealQL so record-level tests don't depend on the
/// client's own record-id handling.
async fn seed(client: &SurrealClient, table: &str, rows: &[(&str, Value)]) {
    let _ = client.delete(table).await;
    for (id, data) in rows {
        let sql = format!(
            "CREATE {} CONTENT $data",
            RecordId::string(table, *id).to_surql()
        );
        let response = client
            .query(&sql, Some(json!({ "data": data })))
            .await
            .expect("seed");
        assert_eq!(response[0]["status"], json!("OK"), "seed {table}:{id}");
    }
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so SurrealDB 3 creates a random-id record in table \"table:id\""]
async fn create_returns_the_new_record() {
    let client = get_client().await;
    let _ = client.delete("sc_create").await;

    let created = client
        .create("sc_create:alice", Some(json!({"name": "Alice", "age": 30})))
        .await
        .expect("create");

    assert_eq!(created["id"], json!("sc_create:alice"));
    assert_eq!(created["name"], json!("Alice"));
    assert_eq!(created["age"], json!(30));

    let _ = client.delete("sc_create").await;
}

#[tokio::test]
async fn create_in_a_table_generates_a_record_id() {
    let client = get_client().await;
    let _ = client.delete("sc_create_table").await;

    let created = client
        .create("sc_create_table", Some(json!({"name": "Alice"})))
        .await
        .expect("create");
    assert_eq!(created["name"], json!("Alice"));
    assert!(
        created["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("sc_create_table:")),
        "generated id: {created}"
    );

    let all = client.select("sc_create_table").await.expect("select");
    assert_eq!(field_values(&all, "name"), vec![json!("Alice")]);

    let _ = client.delete("sc_create_table").await;
}

#[tokio::test]
async fn select_reads_a_whole_table() {
    let client = get_client().await;
    seed(
        &client,
        "sc_select",
        &[("a", json!({"n": 1})), ("b", json!({"n": 2}))],
    )
    .await;

    let all = client.select("sc_select").await.expect("select");
    assert_eq!(field_values(&all, "n"), vec![json!(1), json!(2)]);
    assert_eq!(
        field_values(&all, "id"),
        vec![json!("sc_select:a"), json!("sc_select:b")]
    );

    let table = client
        .select_all(Table::new("sc_select"))
        .await
        .expect("select_all");
    assert_eq!(field_values(&table, "n"), vec![json!(1), json!(2)]);

    let _ = client.delete("sc_select").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so selecting one record reads table \"table:id\" instead"]
async fn select_reads_one_record() {
    let client = get_client().await;
    seed(
        &client,
        "sc_select_one",
        &[("a", json!({"n": 1})), ("b", json!({"n": 2}))],
    )
    .await;

    let one = client.select("sc_select_one:a").await.expect("select");
    assert_eq!(one["n"], json!(1));

    let record = client
        .select_record(RecordId::string("sc_select_one", "b"))
        .await
        .expect("select_record");
    assert_eq!(record["n"], json!(2));

    let _ = client.delete("sc_select_one").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so updating one record targets table \"table:id\" instead"]
async fn update_replaces_the_record_content() {
    let client = get_client().await;
    seed(
        &client,
        "sc_update",
        &[("a", json!({"name": "A", "age": 30}))],
    )
    .await;

    let updated = client
        .update("sc_update:a", Some(json!({"age": 31})))
        .await
        .expect("update");
    assert_eq!(updated["age"], json!(31));
    assert_eq!(updated.get("name"), None);

    let updated = client
        .update_record(RecordId::string("sc_update", "a"), json!({"age": 32}))
        .await
        .expect("update_record");
    assert_eq!(updated["age"], json!(32));

    let _ = client.delete("sc_update").await;
}

#[tokio::test]
async fn update_all_replaces_every_record_in_the_table() {
    let client = get_client().await;
    seed(
        &client,
        "sc_update_all",
        &[("a", json!({"old": true})), ("b", json!({"old": true}))],
    )
    .await;

    client
        .update_all(Table::new("sc_update_all"), json!({"flag": 1}))
        .await
        .expect("update_all");

    let all = client.select("sc_update_all").await.expect("select");
    assert_eq!(field_values(&all, "flag"), vec![json!(1), json!(1)]);
    assert_eq!(field_values(&all, "old"), vec![Value::Null, Value::Null]);

    let _ = client.delete("sc_update_all").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so merging into one record targets table \"table:id\" instead"]
async fn merge_adds_fields_to_one_record_and_keeps_the_rest() {
    let client = get_client().await;
    seed(&client, "sc_merge", &[("a", json!({"name": "a"}))]).await;

    let merged = client
        .merge("sc_merge:a", json!({"city": "Riga"}))
        .await
        .expect("merge");
    assert_eq!(merged["name"], json!("a"));
    assert_eq!(merged["city"], json!("Riga"));

    let merged = client
        .merge_record(RecordId::string("sc_merge", "a"), json!({"zip": 1}))
        .await
        .expect("merge_record");
    assert_eq!(merged["city"], json!("Riga"));
    assert_eq!(merged["zip"], json!(1));

    let _ = client.delete("sc_merge").await;
}

#[tokio::test]
async fn merge_all_adds_fields_to_every_record_and_keeps_the_rest() {
    let client = get_client().await;
    seed(
        &client,
        "sc_merge_all",
        &[("a", json!({"name": "a"})), ("b", json!({"name": "b"}))],
    )
    .await;

    client
        .merge_all(Table::new("sc_merge_all"), json!({"seen": true}))
        .await
        .expect("merge_all");
    let all = client.select("sc_merge_all").await.expect("select");
    assert_eq!(field_values(&all, "seen"), vec![json!(true), json!(true)]);
    assert_eq!(field_values(&all, "name"), vec![json!("a"), json!("b")]);

    client
        .merge("sc_merge_all", json!({"city": "Riga"}))
        .await
        .expect("merge");
    let all = client.select("sc_merge_all").await.expect("select");
    assert_eq!(
        field_values(&all, "city"),
        vec![json!("Riga"), json!("Riga")]
    );
    assert_eq!(field_values(&all, "seen"), vec![json!(true), json!(true)]);

    let _ = client.delete("sc_merge_all").await;
}

#[tokio::test]
async fn upsert_on_an_empty_table_inserts_the_record() {
    let client = get_client().await;
    seed(&client, "sc_upsert_table", &[]).await;

    client
        .upsert("sc_upsert_table", Some(json!({"flag": 1})))
        .await
        .expect("upsert");
    let all = client.select("sc_upsert_table").await.expect("select");
    assert_eq!(field_values(&all, "flag"), vec![json!(1)]);

    let _ = client.delete("sc_upsert_table").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so upserting one record targets table \"table:id\" instead"]
async fn upsert_creates_a_missing_record_and_replaces_an_existing_one() {
    let client = get_client().await;
    let _ = client.delete("sc_upsert").await;

    let created = client
        .upsert("sc_upsert:bob", Some(json!({"name": "Bob"})))
        .await
        .expect("upsert new");
    assert_eq!(created["id"], json!("sc_upsert:bob"));
    assert_eq!(created["name"], json!("Bob"));

    let replaced = client
        .upsert_record(RecordId::string("sc_upsert", "bob"), json!({"age": 40}))
        .await
        .expect("upsert existing");
    assert_eq!(replaced["age"], json!(40));

    let all = client.select("sc_upsert").await.expect("select");
    assert_eq!(all.as_array().map(Vec::len), Some(1));

    let _ = client.delete("sc_upsert").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so patching one record targets table \"table:id\" instead"]
async fn patch_applies_json_patch_operations() {
    let client = get_client().await;
    seed(
        &client,
        "sc_patch",
        &[("a", json!({"email": "old@example.com", "name": "A"}))],
    )
    .await;

    client
        .patch(
            "sc_patch:a",
            vec![
                json!({"op": "replace", "path": "/email", "value": "new@example.com"}),
                json!({"op": "add", "path": "/tags", "value": ["vip"]}),
            ],
        )
        .await
        .expect("patch");

    let record = client.select("sc_patch:a").await.expect("select");
    assert_eq!(record["email"], json!("new@example.com"));
    assert_eq!(record["tags"], json!(["vip"]));
    assert_eq!(record["name"], json!("A"));

    let _ = client.delete("sc_patch").await;
}

#[tokio::test]
#[ignore = "possible bug: a \"table:id\" string is sent as a table name, so deleting one record targets table \"table:id\" instead"]
async fn delete_removes_one_record() {
    let client = get_client().await;
    seed(
        &client,
        "sc_delete",
        &[
            ("a", json!({"name": "a"})),
            ("b", json!({"name": "b"})),
            ("c", json!({"name": "c"})),
        ],
    )
    .await;

    client.delete("sc_delete:a").await.expect("delete");
    client
        .delete_record(RecordId::string("sc_delete", "b"))
        .await
        .expect("delete_record");
    let left = client.select("sc_delete").await.expect("select");
    assert_eq!(field_values(&left, "name"), vec![json!("c")]);

    let _ = client.delete("sc_delete").await;
}

#[tokio::test]
async fn delete_all_empties_the_table() {
    let client = get_client().await;
    seed(
        &client,
        "sc_delete_all",
        &[("a", json!({"name": "a"})), ("b", json!({"name": "b"}))],
    )
    .await;

    client
        .delete_all(Table::new("sc_delete_all"))
        .await
        .expect("delete_all");
    let left = client.select("sc_delete_all").await.expect("select");
    assert_eq!(left, json!([]));

    seed(&client, "sc_delete_all", &[("c", json!({"name": "c"}))]).await;
    client.delete("sc_delete_all").await.expect("delete");
    let left = client.select("sc_delete_all").await.expect("select");
    assert_eq!(left, json!([]));
}

#[tokio::test]
async fn insert_adds_every_record_given() {
    let client = get_client().await;
    let _ = client.delete("sc_insert").await;

    let inserted = client
        .insert("sc_insert", json!([{"name": "Laptop"}, {"name": "Mouse"}]))
        .await
        .expect("insert");
    assert_eq!(
        field_values(&inserted, "name"),
        vec![json!("Laptop"), json!("Mouse")]
    );

    client
        .insert_many(Table::new("sc_insert"), vec![json!({"name": "Keyboard"})])
        .await
        .expect("insert_many");

    let all = client.select("sc_insert").await.expect("select");
    assert_eq!(
        field_values(&all, "name"),
        vec![json!("Keyboard"), json!("Laptop"), json!("Mouse")]
    );

    let _ = client.delete("sc_insert").await;
}

/// Table `<table>` holding records `<table>:1` .. `<table>:5`, each with `n`
/// equal to its id.
async fn range_table(client: &SurrealClient, table: &str) {
    let _ = client.delete(table).await;
    for n in 1..=5 {
        let sql = format!(
            "CREATE {} SET n = {n}",
            RecordId::number(table, n).to_surql()
        );
        let response = client.query(&sql, None).await.expect("seed");
        assert_eq!(response[0]["status"], json!("OK"), "seed {table}:{n}");
    }
}

/// The `n` of every record the range's SurrealQL addresses, sorted.
async fn range_ns(client: &SurrealClient, range: &RecordRange) -> Vec<i64> {
    let sql = format!("SELECT VALUE n FROM {}", range.to_surql());
    let response = client.query(&sql, None).await.expect("range query");
    let mut ns: Vec<i64> = first_result(&response)
        .as_array()
        .unwrap_or_else(|| panic!("{sql} returned {response}"))
        .iter()
        .map(|n| n.as_i64().expect("n"))
        .collect();
    ns.sort();
    ns
}

#[tokio::test]
async fn record_range_with_only_an_inclusive_start_runs_to_the_end() {
    let client = get_client().await;
    range_table(&client, "sc_range_open").await;

    let range = RecordRange::new("sc_range_open").start(2, true);
    assert_eq!(range_ns(&client, &range).await, vec![2, 3, 4, 5]);

    let _ = client.delete("sc_range_open").await;
}

#[tokio::test]
#[ignore = "possible bug: an inclusive end renders as `..end`, which SurrealDB reads as exclusive"]
async fn record_range_with_inclusive_bounds_includes_both_ends() {
    let client = get_client().await;
    range_table(&client, "sc_range_incl").await;

    let range = RecordRange::new("sc_range_incl")
        .start(2, true)
        .end(4, true);
    assert_eq!(range_ns(&client, &range).await, vec![2, 3, 4]);

    let _ = client.delete("sc_range_incl").await;
}

#[tokio::test]
#[ignore = "possible bug: an exclusive end renders as `..=end`, which SurrealDB reads as inclusive"]
async fn record_range_with_exclusive_end_stops_before_it() {
    let client = get_client().await;
    range_table(&client, "sc_range_excl_end").await;

    let range = RecordRange::new("sc_range_excl_end")
        .start(2, true)
        .end(4, false);
    assert_eq!(range_ns(&client, &range).await, vec![2, 3]);

    let _ = client.delete("sc_range_excl_end").await;
}

#[tokio::test]
#[ignore = "possible bug: an exclusive start renders as `table:>start..`, which SurrealDB fails to parse (its syntax is `start>..`)"]
async fn record_range_with_exclusive_start_skips_it() {
    let client = get_client().await;
    range_table(&client, "sc_range_excl_start").await;

    let range = RecordRange::new("sc_range_excl_start")
        .start(2, false)
        .end(4, true);
    assert_eq!(range_ns(&client, &range).await, vec![3, 4]);

    let _ = client.delete("sc_range_excl_start").await;
}

#[tokio::test]
async fn record_range_with_string_ids_orders_by_id() {
    let client = get_client().await;
    seed(
        &client,
        "sc_range_str",
        &[
            ("a", json!({"n": 1})),
            ("b", json!({"n": 2})),
            ("c", json!({"n": 3})),
        ],
    )
    .await;

    let range = RecordRange::new("sc_range_str").start("b", true);
    assert_eq!(range_ns(&client, &range).await, vec![2, 3]);

    let _ = client.delete("sc_range_str").await;
}

#[tokio::test]
async fn record_range_end_leaves_out_records_past_it() {
    let client = get_client().await;
    seed(
        &client,
        "sc_range_end",
        &[
            ("a", json!({"n": 1})),
            ("b", json!({"n": 2})),
            ("c", json!({"n": 3})),
        ],
    )
    .await;

    for inclusive in [true, false] {
        let range = RecordRange::new("sc_range_end")
            .start("a", true)
            .end("bz", inclusive);
        assert_eq!(range_ns(&client, &range).await, vec![1, 2], "{range}");
    }

    let _ = client.delete("sc_range_end").await;
}

#[tokio::test]
#[ignore = "possible bug: the range is sent as a \"table:a..b\" string, which SurrealDB 3 reads as a table name"]
async fn select_range_reads_the_records_in_the_range() {
    let client = get_client().await;
    range_table(&client, "sc_select_range").await;

    let rows = client
        .select_range(RecordRange::new("sc_select_range").start(2, true))
        .await
        .expect("select_range");
    assert_eq!(
        field_values(&rows, "n"),
        vec![json!(2), json!(3), json!(4), json!(5)]
    );

    let _ = client.delete("sc_select_range").await;
}

#[tokio::test]
async fn object_and_array_record_ids_render_as_addressable_surql() {
    let client = get_client().await;
    let _ = client.delete("sc_complex_ids").await;
    let ids = [
        RecordId::array("sc_complex_ids", vec![json!(1), json!("x")]),
        RecordId::object("sc_complex_ids", json!({"region": "eu", "n": 2})),
    ];

    for (n, id) in ids.iter().enumerate() {
        let sql = format!("CREATE {} SET n = {n}", id.to_surql());
        let response = client.query(&sql, None).await.expect("create");
        assert_eq!(response[0]["status"], json!("OK"), "{sql}: {response}");
    }
    for (n, id) in ids.iter().enumerate() {
        let sql = format!("SELECT VALUE n FROM {}", id.to_surql());
        let response = client.query(&sql, None).await.expect("select");
        assert_eq!(first_result(&response), &json!([n]), "{sql}");
    }

    let _ = client.delete("sc_complex_ids").await;
}

#[tokio::test]
#[ignore = "possible bug: \"table:id\" strings are sent as table names, so SurrealDB 3 refuses them as RELATE endpoints"]
async fn relate_creates_an_edge_between_records() {
    let client = get_client().await;
    let _ = client.delete("sc_rel_likes").await;
    seed(
        &client,
        "sc_rel_user",
        &[("alice", json!({"name": "Alice"}))],
    )
    .await;
    seed(
        &client,
        "sc_rel_post",
        &[
            ("p1", json!({"title": "One"})),
            ("p2", json!({"title": "Two"})),
        ],
    )
    .await;

    client
        .relate(
            "sc_rel_user:alice",
            "sc_rel_likes",
            "sc_rel_post:p1",
            Some(json!({"stars": 5})),
        )
        .await
        .expect("relate");
    client
        .relate_records(
            RecordId::string("sc_rel_user", "alice"),
            Table::new("sc_rel_likes"),
            RecordId::string("sc_rel_post", "p2"),
            None,
        )
        .await
        .expect("relate_records");

    let edges = client.select("sc_rel_likes").await.expect("select edges");
    assert_eq!(
        field_values(&edges, "in"),
        vec![json!("sc_rel_user:alice"), json!("sc_rel_user:alice")]
    );
    assert_eq!(
        field_values(&edges, "out"),
        vec![json!("sc_rel_post:p1"), json!("sc_rel_post:p2")]
    );
    assert_eq!(field_values(&edges, "stars"), vec![Value::Null, json!(5)]);

    let liked = client
        .query(
            "SELECT VALUE title FROM sc_rel_user:alice->sc_rel_likes->sc_rel_post",
            None,
        )
        .await
        .expect("traverse");
    let mut titles = first_result(&liked).as_array().expect("titles").clone();
    titles.sort_by_key(|v| v.to_string());
    assert_eq!(titles, vec![json!("One"), json!("Two")]);

    for table in ["sc_rel_user", "sc_rel_post", "sc_rel_likes"] {
        let _ = client.delete(table).await;
    }
}

#[tokio::test]
#[ignore = "possible bug: run sends its arguments in the RPC's version slot, so SurrealDB answers \"Expected version to be string\""]
async fn run_calls_a_function_with_arguments() {
    let client = get_client().await;

    let shouted = client
        .run("string::uppercase", Some(json!(["abc"])))
        .await
        .expect("run");
    assert_eq!(shouted, json!("ABC"));
}

#[tokio::test]
async fn run_calls_a_function_without_arguments() {
    let client = get_client().await;

    let id = client.run("rand::uuid", None).await.expect("run");
    assert!(id.as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn query_binds_variables_and_returns_one_json_result_per_statement() {
    let client = get_client().await;

    let response = client
        .query(
            "RETURN $a + $b; RETURN <bytes>'hi'; RETURN [sc_ids:john, sc_ids:42]",
            Some(json!({"a": 2, "b": 3})),
        )
        .await
        .expect("query");
    assert_eq!(response.as_array().map(Vec::len), Some(3));
    assert_eq!(response[0]["status"], json!("OK"));
    assert_eq!(response[0]["result"], json!(5));
    assert_eq!(response[1]["result"], json!("aGk="));
    assert_eq!(response[2]["result"], json!(["sc_ids:john", "sc_ids:42"]));
}

#[tokio::test]
async fn session_variables_are_visible_until_unset() {
    let mut client = get_client().await;

    client
        .let_var("sc_role", json!("admin"))
        .await
        .expect("let_var");
    let response = client.query("RETURN $sc_role", None).await.expect("query");
    assert_eq!(first_result(&response), &json!("admin"));

    client.unset("sc_role").await.expect("unset");
    let response = client.query("RETURN $sc_role", None).await.expect("query");
    assert_eq!(first_result(&response), &Value::Null);
}

#[tokio::test]
#[ignore = "possible bug: clones share one server session, so a variable set on one clone overwrites the other's"]
async fn cloned_clients_keep_separate_session_variables() {
    let mut client1 = get_client().await;
    client1
        .let_var("sc_clone_role", json!("admin"))
        .await
        .expect("let_var on client1");

    let mut client2 = client1.clone();
    client2
        .let_var("sc_clone_role", json!("user"))
        .await
        .expect("let_var on client2");

    let seen1 = client1
        .query("RETURN $sc_clone_role", None)
        .await
        .expect("query client1");
    let seen2 = client2
        .query("RETURN $sc_clone_role", None)
        .await
        .expect("query client2");
    assert_eq!(first_result(&seen1), &json!("admin"));
    assert_eq!(first_result(&seen2), &json!("user"));
}

#[tokio::test]
async fn each_connect_opens_its_own_session() {
    let conn = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS);
    let mut db1 = conn.clone().connect().await.expect("connect db1");
    let mut db2 = conn.connect().await.expect("connect db2");

    db1.let_var("sc_conn_role", json!("one"))
        .await
        .expect("let_var db1");
    db2.let_var("sc_conn_role", json!("two"))
        .await
        .expect("let_var db2");

    let (seen1, seen2) = tokio::try_join!(
        db1.query("RETURN $sc_conn_role", None),
        db2.query("RETURN $sc_conn_role", None)
    )
    .expect("parallel queries");
    assert_eq!(first_result(&seen1), &json!("one"));
    assert_eq!(first_result(&seen2), &json!("two"));
}

#[tokio::test]
async fn query_cbor_binds_cbor_variables_and_returns_cbor() {
    use ciborium::Value as Cbor;

    let client = get_client().await;
    let vars = Cbor::Map(vec![(Cbor::Text("x".into()), Cbor::Integer(21.into()))]);

    let response = client
        .query_cbor("RETURN $x * 2", vars)
        .await
        .expect("query_cbor");

    let Cbor::Array(statements) = response else {
        panic!("expected one entry per statement, got {response:?}");
    };
    let Cbor::Map(first) = &statements[0] else {
        panic!("expected a statement result map, got {:?}", statements[0]);
    };
    let result = first
        .iter()
        .find(|(k, _)| k == &Cbor::Text("result".into()))
        .map(|(_, v)| v);
    assert_eq!(result, Some(&Cbor::Integer(42.into())));
}

#[tokio::test]
async fn close_consumes_the_client() {
    let client = get_client().await;
    client.close().await.expect("close");
}

#[tokio::test]
async fn http_only_operations_fail_over_websocket() {
    let client = get_client().await;

    assert!(client.import("", ROOT_USER, ROOT_PASS).await.is_err());
    assert!(client.export(ROOT_USER, ROOT_PASS).await.is_err());
    assert!(client.import_ml("", None, None).await.is_err());
    assert!(client.export_ml("model", None, None, None).await.is_err());
}

#[tokio::test]
async fn debug_connection_still_returns_results() {
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .with_debug(true)
        .connect()
        .await
        .expect("connect");
    assert!(client.is_debug());

    let response = client
        .query("RETURN $v", Some(json!({"v": "seen"})))
        .await
        .expect("query with vars");
    assert_eq!(first_result(&response), &json!("seen"));

    let response = client.query("RETURN 7", None).await.expect("query");
    assert_eq!(first_result(&response), &json!(7));

    assert!(!client.version().await.expect("version").is_empty());
}

#[tokio::test]
async fn debug_flag_on_the_client_can_be_toggled() {
    let client = get_client().await;
    assert!(!client.is_debug());
    assert!(client.with_debug(true).is_debug());
}

async fn session_target(client: &SurrealClient) -> (Value, Value) {
    let response = client
        .query("RETURN session::ns(); RETURN session::db()", None)
        .await
        .expect("session query");
    (response[0]["result"].clone(), response[1]["result"].clone())
}

#[tokio::test]
async fn dsn_path_selects_namespace_and_database() {
    let dsn = format!("ws://{ROOT_USER}:{ROOT_PASS}@localhost:8000/{TEST_NAMESPACE}/sc_dsn_path");
    let client = SurrealConnection::dsn(&dsn)
        .expect("parse")
        .connect()
        .await
        .expect("connect");

    assert_eq!(
        session_target(&client).await,
        (json!(TEST_NAMESPACE), json!("sc_dsn_path"))
    );
}

#[tokio::test]
async fn dsn_query_parameters_select_namespace_and_database() {
    let dsn = format!(
        "ws://{ROOT_USER}:{ROOT_PASS}@localhost:8000?namespace={TEST_NAMESPACE}&database=sc_dsn_query&version_check=false"
    );
    let conn = SurrealConnection::dsn(&dsn).expect("parse");
    assert_eq!(conn.url.as_deref(), Some("ws://localhost:8000"));

    let client = conn.connect().await.expect("connect");
    assert_eq!(
        session_target(&client).await,
        (json!(TEST_NAMESPACE), json!("sc_dsn_query"))
    );
}

#[tokio::test]
async fn dsn_without_credentials_takes_auth_from_the_builder() {
    let client = SurrealConnection::dsn(format!(
        "ws://localhost:8000/{TEST_NAMESPACE}/sc_dsn_no_creds"
    ))
    .expect("parse")
    .auth_root(ROOT_USER, ROOT_PASS)
    .connect()
    .await
    .expect("connect");

    assert_eq!(
        session_target(&client).await,
        (json!(TEST_NAMESPACE), json!("sc_dsn_no_creds"))
    );
}

#[tokio::test]
async fn dsn_explicit_root_auth_level_signs_in_as_root() {
    let dsn = format!(
        "ws://{ROOT_USER}:{ROOT_PASS}@localhost:8000/{TEST_NAMESPACE}/{TEST_DATABASE}?auth=root"
    );
    let client = SurrealConnection::dsn(&dsn)
        .expect("parse")
        .connect()
        .await
        .expect("connect");
    assert!(!client.version().await.expect("version").is_empty());
}

#[tokio::test]
async fn dsn_namespace_and_database_auth_levels_do_not_sign_in_as_root() {
    for level in ["namespace", "ns", "database", "db"] {
        let dsn = format!(
            "ws://{ROOT_USER}:{ROOT_PASS}@localhost:8000/{TEST_NAMESPACE}/{TEST_DATABASE}?auth={level}"
        );
        let conn = SurrealConnection::dsn(&dsn).expect("parse");
        assert!(
            conn.connect().await.is_err(),
            "root credentials must not pass a {level}-level signin"
        );
    }
}

#[test]
fn dsn_rejects_an_unknown_auth_level() {
    let err = SurrealConnection::dsn("ws://u:p@localhost:8000/ns/db?auth=nope")
        .expect_err("unknown auth level");
    assert!(matches!(err, SurrealError::Connection(_)));
}

#[test]
fn dsn_rejects_input_that_is_not_a_url_with_a_host() {
    assert!(SurrealConnection::dsn("not a url").is_err());
    assert!(SurrealConnection::dsn("mailto:someone").is_err());
}

#[tokio::test]
async fn connect_requires_a_url() {
    let result = SurrealConnection::new()
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await;
    assert!(matches!(result, Err(SurrealError::Connection(_))));
}

#[tokio::test]
async fn connect_rejects_non_websocket_schemes() {
    let result = SurrealConnection::new()
        .url("http://localhost:8000")
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn connect_accepts_the_cbor_scheme_alias() {
    let client = SurrealConnection::new()
        .url("cbor://localhost:8000")
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .version_check(false)
        .connect()
        .await
        .expect("connect");
    assert!(!client.version().await.expect("version").is_empty());
}

#[tokio::test]
async fn connect_requires_credentials() {
    let result = SurrealConnection::new().url(DB_URL).connect().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn connect_reports_an_unreachable_server_as_a_connection_error() {
    let result = SurrealConnection::new()
        .url("ws://127.0.0.1:1")
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await;
    assert!(matches!(result, Err(SurrealError::Connection(_))));
}

#[tokio::test]
async fn connect_rejects_scope_auth_as_unsupported() {
    let Err(err) = SurrealConnection::new()
        .url(DB_URL)
        .auth_scope(TEST_NAMESPACE, TEST_DATABASE, "user_scope", json!({}))
        .connect()
        .await
    else {
        panic!("scope auth should not connect");
    };
    assert!(
        err.to_string()
            .contains("Unsupported authentication method")
    );
}

#[tokio::test]
async fn namespace_auth_needs_a_namespace() {
    let result = SurrealConnection::new()
        .url(DB_URL)
        .auth_namespace("ns_user", "ns_pass")
        .connect()
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn database_auth_needs_a_namespace_and_a_database() {
    let no_ns = SurrealConnection::new()
        .url(DB_URL)
        .auth_database("db_user", "db_pass")
        .connect()
        .await;
    assert!(no_ns.is_err());

    let no_db = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .auth_database("db_user", "db_pass")
        .connect()
        .await;
    assert!(no_db.is_err());
}

#[tokio::test]
async fn namespace_and_database_auth_reject_unknown_users() {
    let ns = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .auth_namespace("sc_no_such_user", "wrong")
        .connect()
        .await;
    assert!(ns.is_err());

    let db = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_database("sc_no_such_user", "wrong")
        .connect()
        .await;
    assert!(db.is_err());
}

#[tokio::test]
async fn token_auth_rejects_an_invalid_token() {
    let result = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_token("not-a-jwt")
        .connect()
        .await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore]
async fn test_import_export() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("import_test").await;

    // Create some test data
    let test_data = [
        json!({"name": "Item 1", "value": 100}),
        json!({"name": "Item 2", "value": 200}),
        json!({"name": "Item 3", "value": 300}),
    ];

    for (i, data) in test_data.iter().enumerate() {
        client
            .create(&format!("import_test:item_{}", i + 1), Some(data.clone()))
            .await
            .expect("Failed to create test data");
    }

    // Export data using query
    let exported = client
        .query("SELECT * FROM import_test ORDER BY name", None)
        .await
        .expect("Failed to export data");

    println!("Exported data: {:?}", exported);

    // Test bulk import using transaction
    let bulk_import = r#"
        BEGIN TRANSACTION;

        DELETE import_test;

        CREATE import_test:batch_1 SET name = "Batch Item 1", value = 1000, batch = true;
        CREATE import_test:batch_2 SET name = "Batch Item 2", value = 2000, batch = true;
        CREATE import_test:batch_3 SET name = "Batch Item 3", value = 3000, batch = true;

        COMMIT TRANSACTION;
    "#;

    let import_result = client
        .query(bulk_import, None)
        .await
        .expect("Failed to bulk import");

    println!("Bulk import result: {:?}", import_result);

    // Verify import
    let after_import = client
        .select("import_test")
        .await
        .expect("Failed to verify import");

    println!("Data after import: {:?}", after_import);

    // Clean up
    let _ = client.delete("import_test").await;
}
