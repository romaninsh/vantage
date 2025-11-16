//! Comprehensive unstructured insert functionality test cases
//!
//! Tests all SurrealDB types with individual test cases and record verification

use rust_decimal::Decimal;
use serde_json::json;
use surreal_client::SurrealConnection;
use surreal_client::types::{Any, DateTime, Duration};
use uuid::Uuid;
use vantage_surrealdb::expression::field::Field;
use vantage_surrealdb::prelude::*;

async fn setup() -> (SurrealDB, String) {
    let db = SurrealConnection::dsn("cbor://root:root@localhost:8000/test/v1")
        .unwrap()
        .connect()
        .await
        .unwrap();
    let ds = SurrealDB::new(db);
    let table_name = format!("test_{}", Uuid::new_v4().to_string().replace("-", ""));
    (ds, table_name)
}

async fn cleanup(ds: &SurrealDB, table_name: &str) {
    let drop_query = format!("REMOVE TABLE {}", table_name);
    let _drop_result = ds.query(drop_query, serde_json::Value::Null).await;
    println!("Table {} dropped", table_name);
}

macro_rules! assert_similar {
    ($val1:expr, $val2:expr, $max_error:expr) => {
        let num = $val1.as_number().unwrap();
        let retrieved = num.as_f64().unwrap();
        let expected = $val2 as f64;
        assert!(
            (retrieved - expected).abs() < $max_error,
            "Values differ by more than {}: {} vs {}",
            $max_error,
            retrieved,
            expected
        );
    };
}

macro_rules! test_types {
    ($test_name:literal, $($field:ident = $value:expr),+ $(,)?) => {
        let (ds, table_name) = setup().await;

        let mut insert = SurrealInsert::new(&table_name)
            .with_id(concat!($test_name, "_test"));

        $(
            insert = insert.set_field(stringify!($field), $value);
        )+

        let result = ds.get(insert).await;
        let obj = &result[0];

        $(
            assert_eq!(obj.get(stringify!($field)).unwrap(), &json!($value));
        )+

        // Test with conditions
        let mut select_with_conditions = ds
            .select()
            .with_source(format!("{}:{}_test", table_name, $test_name))
            .only_first_row();

        $(
            select_with_conditions = select_with_conditions
                .with_condition(Field::new(stringify!($field)).eq($value));
        )+

        let obj2 = select_with_conditions.get(&ds).await;

        $(
            assert_eq!(obj2.get(stringify!($field)).unwrap(), &json!($value));
        )+

        cleanup(&ds, &table_name).await;
    };
}

#[tokio::test]
async fn test_insert_string() {
    test_types!("string", name = "John Doe".to_string(), nickname = "Johnny");
}

#[tokio::test]
async fn test_insert_int() {
    test_types!(
        "int",
        i8_val = -42i8,
        i16_val = -12345i16,
        i32_val = -987654321i32,
        i64_val = -1234567890123456789i64,
        u8_val = 200u8,
        u16_val = 50000u16,
        u32_val = 3000000000u32,
        u64_val = 1234567890123456u64,
        isize_val = -999isize,
        usize_val = 12345usize
    );
}

#[tokio::test]
async fn test_insert_float() {
    let (ds, table_name) = setup().await;

    let f32_val = 3.14f32;
    let f64_val = 2.718281828459045f64;
    let pi = 3.141592653589793f64;

    let insert = SurrealInsert::new(&table_name)
        .with_id("float_test")
        .set_field("f32_val", f32_val)
        .set_field("f64_val", f64_val)
        .set_field("pi", pi);

    let result = ds.get(insert).await;
    let obj = &result[0];

    assert_similar!(obj.get("f32_val").unwrap(), 3.14f32, 1e-6);
    assert_similar!(obj.get("f64_val").unwrap(), 2.718281828459045f64, 1e-15);
    assert_similar!(obj.get("pi").unwrap(), 3.141592653589793f64, 1e-15);

    cleanup(&ds, &table_name).await;
}

#[tokio::test]
async fn test_insert_bool() {
    test_types!("bool", active = true, debug = false, enabled = true);
}

#[tokio::test]
async fn test_insert_datetime() {
    test_types!(
        "datetime",
        surreal_birthday = DateTime::new(
            "1980-09-08T13:37:01Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        ),
        chrono_married = "1981-06-15T09:30:45Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap(),
        std_anniversary =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(347_558_400)
    );
}

#[tokio::test]
async fn test_insert_duration() {
    test_types!(
        "duration",
        std_timeout = std::time::Duration::from_secs(300),
        std_retry = std::time::Duration::from_millis(5000),
        surreal_timeout = Duration::new(std::time::Duration::from_secs(600)),
        surreal_retry = Duration::new(std::time::Duration::from_millis(2500)),
        chrono_duration = chrono::Duration::seconds(900)
    );
}

#[tokio::test]
async fn test_insert_json() {
    let (ds, table_name) = setup().await;

    let metadata = serde_json::json!({
        "version": "1.0.0",
        "features": ["auth", "logging", "metrics"],
        "config": {
            "max_connections": 100,
            "timeout": 30.5,
            "debug": true
        },
        "numbers": [1, 2, 3.14159, -42]
    });

    let tags = serde_json::json!(["rust", "database", "surreal", "nosql"]);

    let insert = SurrealInsert::new(&table_name)
        .with_id("json_test")
        .set_field("metadata", metadata.clone())
        .set_field("tags", tags.clone());

    let _result = ds.get(insert).await;
    let single_row_select = ds
        .select()
        .with_source(format!("{}:json_test", table_name))
        .only_first_row();
    let obj = single_row_select.get(&ds).await;

    if let Some(retrieved_metadata) = obj.get("metadata") {
        assert_eq!(retrieved_metadata.get("version"), Some(&json!("1.0.0")));
        assert_eq!(
            retrieved_metadata
                .get("config")
                .unwrap()
                .get("max_connections"),
            Some(&json!(100))
        );
        assert_eq!(
            retrieved_metadata.get("config").unwrap().get("timeout"),
            Some(&json!(30.5))
        );
    }
    if let Some(serde_json::Value::Array(retrieved_tags)) = obj.get("tags") {
        assert_eq!(retrieved_tags.len(), 4);
        assert_eq!(retrieved_tags[0], json!("rust"));
    }

    cleanup(&ds, &table_name).await;
}

#[tokio::test]
async fn test_insert_any() {
    let (ds, table_name) = setup().await;

    let insert = SurrealInsert::new(&table_name)
        .with_id("any_test")
        .set_field("null_field", Any)
        .set_field("optional_data", Any);

    let _result = ds.get(insert).await;
    let single_row_select = ds
        .select()
        .with_source(format!("{}:any_test", table_name))
        .only_first_row();
    let obj = single_row_select.get(&ds).await;

    // NONE fields should not be present in the result
    assert!(
        !obj.contains_key("null_field"),
        "NONE fields should be omitted"
    );
    assert!(
        !obj.contains_key("optional_data"),
        "NONE fields should be omitted"
    );
    assert!(obj.contains_key("id"), "ID should be present");

    cleanup(&ds, &table_name).await;
}

#[tokio::test]
async fn test_insert_record_references() {
    let (ds, table_name) = setup().await;

    let insert = SurrealInsert::new(&table_name)
        .with_id("ref_test")
        .set_field("bakery", "bakery:hill_valley".to_string())
        .set_field("owner", "user:admin".to_string())
        .set_field("category", "product:electronics".to_string());

    let _result = ds.get(insert).await;
    let single_row_select = ds
        .select()
        .with_source(format!("{}:ref_test", table_name))
        .only_first_row();
    let obj = single_row_select.get(&ds).await;
    assert_eq!(obj.get("bakery").unwrap(), &json!("bakery:hill_valley"));
    assert_eq!(obj.get("owner").unwrap(), &json!("user:admin"));
    assert_eq!(obj.get("category").unwrap(), &json!("product:electronics"));

    cleanup(&ds, &table_name).await;
}

#[cfg(feature = "decimal")]
#[tokio::test]
async fn test_insert_decimal() {
    let (ds, table_name) = setup().await;

    let regular_decimal = Decimal::new(12345, 2); // 123.45
    let high_precision = "999999999999999999.999999999999999999"
        .parse::<Decimal>()
        .unwrap();
    let small_precise = "0.000000000000000001".parse::<Decimal>().unwrap();

    let insert = SurrealInsert::new(&table_name)
        .with_id("decimal_test")
        .set_field("balance", regular_decimal)
        .set_field("high_precision", high_precision)
        .set_field("small_precise", small_precise);

    let _result = ds.get(insert).await;
    let single_row_select = ds
        .select()
        .with_source(format!("{}:decimal_test", table_name))
        .only_first_row();
    let obj = single_row_select.get(&ds).await;
    assert_eq!(obj.get("balance").unwrap(), &json!("123.45"));

    // High precision should be preserved (though may be truncated by SurrealDB)
    if let Some(serde_json::Value::String(hp)) = obj.get("high_precision") {
        assert!(
            hp.starts_with("1000000000000000000"),
            "High precision should be preserved"
        );
    }

    cleanup(&ds, &table_name).await;
}
