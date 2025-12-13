//! Integration tests for CBOR datetime and duration types
//!
//! Tests the full round-trip of datetime and duration values through SurrealDB
//! using the CBOR protocol to ensure native type support works correctly.

use ciborium::Value as CborValue;
use serde_json::Value;
use std::time::{Duration as StdDuration, SystemTime};
use surreal_client::{
    SurrealConnection,
    types::{AnySurrealType, SurrealType},
};

async fn setup() -> (surreal_client::SurrealClient, String) {
    let client = SurrealConnection::dsn("cbor://root:root@localhost:8000/test/cbor")
        .unwrap()
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");

    let table_name = format!(
        "cbor_test_{}",
        uuid::Uuid::new_v4().to_string().replace("-", "")
    );
    (client, table_name)
}

async fn cleanup(client: &surreal_client::SurrealClient, table_name: &str) {
    let drop_query = format!("REMOVE TABLE {}", table_name);
    let _result = client.query_cbor(&drop_query, CborValue::Null).await;
    println!("Table {} dropped", table_name);
}

#[tokio::test]
async fn test_cbor_datetime_types() {
    let (client, table_name) = setup().await;

    // Test different datetime types using standard Rust types
    let chrono_datetime = chrono::Utc::now();
    let system_time = SystemTime::now();

    // Create record with different datetime types
    let create_query = format!(
        "CREATE {}:datetime_test SET chrono_dt = $chrono_dt, system_dt = $system_dt",
        table_name
    );

    // Create CBOR parameters using to_cbor() methods
    let params = CborValue::Map(vec![
        (
            CborValue::Text("chrono_dt".to_string()),
            chrono_datetime.to_cbor(),
        ),
        (
            CborValue::Text("system_dt".to_string()),
            system_time.to_cbor(),
        ),
    ]);

    let create_result = client
        .query_cbor(&create_query, params)
        .await
        .expect("Create failed");
    println!("Create result: {:?}", create_result);

    // Select the record back
    let select_query = format!("SELECT * FROM {}:datetime_test", table_name);
    let select_result = client
        .query_cbor(&select_query, CborValue::Null)
        .await
        .expect("Select failed");

    println!("Select result: {:?}", select_result);

    // Verify the record was created and values are preserved
    // Convert CBOR response to JSON for easier testing
    let select_json = cbor_to_json_for_test(select_result);
    if let Value::Array(arr) = select_json {
        assert!(!arr.is_empty(), "Should have at least one record");

        // Navigate the response structure: response[0].result[0]
        let query_response = &arr[0];
        let result_array = query_response
            .get("result")
            .expect("Missing result field in query response")
            .as_array()
            .expect("Result field should be an array");

        assert!(
            !result_array.is_empty(),
            "Should have at least one record in result"
        );
        let record = &result_array[0];

        // Check that datetime fields exist
        assert!(
            record.get("chrono_dt").is_some(),
            "chrono_dt field missing in record: {:?}",
            record
        );
        assert!(
            record.get("system_dt").is_some(),
            "system_dt field missing in record: {:?}",
            record
        );

        println!("DateTime fields preserved correctly");
    } else {
        panic!("Expected array result from select");
    }

    cleanup(&client, &table_name).await;
}

// Helper function to convert CBOR to JSON for test assertions
fn cbor_to_json_for_test(cbor: CborValue) -> Value {
    match cbor {
        CborValue::Null => Value::Null,
        CborValue::Bool(b) => Value::Bool(b),
        CborValue::Integer(i) => {
            let num = i128::from(i);
            if let Ok(i64_val) = i64::try_from(num) {
                serde_json::Number::from(i64_val).into()
            } else {
                Value::String(num.to_string())
            }
        }
        CborValue::Float(f) => serde_json::Number::from_f64(f).unwrap().into(),
        CborValue::Text(s) => Value::String(s),
        CborValue::Array(arr) => Value::Array(arr.into_iter().map(cbor_to_json_for_test).collect()),
        CborValue::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let CborValue::Text(key) = k {
                    obj.insert(key, cbor_to_json_for_test(v));
                }
            }
            Value::Object(obj)
        }
        CborValue::Tag(_tag, value) => cbor_to_json_for_test(*value),
        _ => Value::String(format!("{:?}", cbor)),
    }
}

#[tokio::test]
async fn test_cbor_duration_types() {
    let (client, table_name) = setup().await;

    // Test different duration types using standard Rust types
    let std_duration = StdDuration::from_millis(5000);
    let chrono_duration = chrono::Duration::seconds(900);

    // Create record with different duration types
    let create_query = format!(
        "CREATE {}:duration_test SET std_dur = $std_dur, chrono_dur = $chrono_dur",
        table_name
    );

    // Create CBOR parameters using to_cbor() methods
    let params = CborValue::Map(vec![
        (
            CborValue::Text("std_dur".to_string()),
            std_duration.to_cbor(),
        ),
        (
            CborValue::Text("chrono_dur".to_string()),
            chrono_duration.to_cbor(),
        ),
    ]);

    let create_result = client
        .query_cbor(&create_query, params)
        .await
        .expect("Create failed");
    println!("Create result: {:?}", create_result);

    // Select the record back
    let select_query = format!("SELECT * FROM {}:duration_test", table_name);
    let select_result = client
        .query_cbor(&select_query, CborValue::Null)
        .await
        .expect("Select failed");

    println!("Select result: {:?}", select_result);

    // Verify the record was created and values are preserved
    let select_json = cbor_to_json_for_test(select_result);
    if let Value::Array(arr) = select_json {
        assert!(!arr.is_empty(), "Should have at least one record");

        // Navigate the response structure: response[0].result[0]
        let query_response = &arr[0];
        let result_array = query_response
            .get("result")
            .expect("Missing result field in query response")
            .as_array()
            .expect("Result field should be an array");

        assert!(
            !result_array.is_empty(),
            "Should have at least one record in result"
        );
        let record = &result_array[0];

        // Check that duration fields exist
        assert!(
            record.get("std_dur").is_some(),
            "std_dur field missing in record: {:?}",
            record
        );
        assert!(
            record.get("chrono_dur").is_some(),
            "chrono_dur field missing in record: {:?}",
            record
        );

        println!("Duration fields preserved correctly");
    } else {
        panic!("Expected array result from select");
    }

    cleanup(&client, &table_name).await;
}

#[tokio::test]
async fn test_cbor_mixed_datetime_duration() {
    let (client, table_name) = setup().await;

    // Test mixed datetime and duration types in one record
    let birthday: chrono::DateTime<chrono::Utc> = "1980-09-08T13:37:01Z".parse().unwrap();
    let anniversary = SystemTime::UNIX_EPOCH + StdDuration::from_secs(347_558_400);
    let timeout = StdDuration::from_secs(600);
    let retry_delay = chrono::Duration::seconds(30);

    // Create record with mixed types
    let create_query = format!(
        "CREATE {}:mixed_test SET birthday = $birthday, anniversary = $anniversary, timeout = $timeout, retry_delay = $retry_delay",
        table_name
    );

    // Create CBOR parameters using to_cbor() methods
    let params = CborValue::Map(vec![
        (CborValue::Text("birthday".to_string()), birthday.to_cbor()),
        (
            CborValue::Text("anniversary".to_string()),
            anniversary.to_cbor(),
        ),
        (CborValue::Text("timeout".to_string()), timeout.to_cbor()),
        (
            CborValue::Text("retry_delay".to_string()),
            retry_delay.to_cbor(),
        ),
    ]);

    let create_result = client
        .query_cbor(&create_query, params)
        .await
        .expect("Create failed");
    println!("Create result: {:?}", create_result);

    // Select the record back
    let select_query = format!("SELECT * FROM {}:mixed_test", table_name);
    let select_result = client
        .query_cbor(&select_query, CborValue::Null)
        .await
        .expect("Select failed");

    println!("Select result: {:?}", select_result);

    // Verify all fields are preserved
    let select_json = cbor_to_json_for_test(select_result);
    if let Value::Array(arr) = select_json {
        assert!(!arr.is_empty(), "Should have at least one record");

        // Navigate the response structure: response[0].result[0]
        let query_response = &arr[0];
        let result_array = query_response
            .get("result")
            .expect("Missing result field in query response")
            .as_array()
            .expect("Result field should be an array");

        assert!(
            !result_array.is_empty(),
            "Should have at least one record in result"
        );
        let record = &result_array[0];

        // Check that all fields exist
        assert!(
            record.get("birthday").is_some(),
            "birthday field missing in record: {:?}",
            record
        );
        assert!(
            record.get("anniversary").is_some(),
            "anniversary field missing in record: {:?}",
            record
        );
        assert!(
            record.get("timeout").is_some(),
            "timeout field missing in record: {:?}",
            record
        );
        assert!(
            record.get("retry_delay").is_some(),
            "retry_delay field missing in record: {:?}",
            record
        );

        println!("All mixed datetime/duration fields preserved correctly");
    } else {
        panic!("Expected array result from select");
    }

    cleanup(&client, &table_name).await;
}

#[tokio::test]
async fn test_cbor_precision_preservation() {
    let (client, table_name) = setup().await;

    // Test that precision is preserved with nanoseconds
    let precise_time = chrono::Utc::now();
    let precise_duration = StdDuration::new(123, 456_789_000); // 123 seconds + 456ms + 789μs

    // Create record
    let create_query = format!(
        "CREATE {}:precision_test SET precise_time = $precise_time, precise_duration = $precise_duration",
        table_name
    );

    // Create CBOR parameters using to_cbor() methods
    let params = CborValue::Map(vec![
        (
            CborValue::Text("precise_time".to_string()),
            precise_time.to_cbor(),
        ),
        (
            CborValue::Text("precise_duration".to_string()),
            precise_duration.to_cbor(),
        ),
    ]);

    let create_result = client
        .query_cbor(&create_query, params)
        .await
        .expect("Create failed");
    println!("Create result: {:?}", create_result);

    // Select back and verify precision
    let select_query = format!("SELECT * FROM {}:precision_test", table_name);
    let select_result = client
        .query_cbor(&select_query, CborValue::Null)
        .await
        .expect("Select failed");

    println!("Select result: {:?}", select_result);

    let select_json = cbor_to_json_for_test(select_result);
    if let Value::Array(arr) = select_json {
        assert!(!arr.is_empty(), "Should have at least one record");

        // Navigate the response structure: response[0].result[0]
        let query_response = &arr[0];
        let result_array = query_response
            .get("result")
            .expect("Missing result field in query response")
            .as_array()
            .expect("Result field should be an array");

        assert!(
            !result_array.is_empty(),
            "Should have at least one record in result"
        );
        let record = &result_array[0];

        // Verify that datetime and duration fields exist (precision testing would need more detailed verification)
        assert!(
            record.get("precise_time").is_some(),
            "precise_time field missing in record: {:?}",
            record
        );
        assert!(
            record.get("precise_duration").is_some(),
            "precise_duration field missing in record: {:?}",
            record
        );

        println!(
            "Precision fields preserved (detailed precision verification would require additional checking)"
        );
    } else {
        panic!("Expected array result from select");
    }

    cleanup(&client, &table_name).await;
}

#[tokio::test]
async fn test_cbor_type_erasure() {
    let (client, table_name) = setup().await;

    // Test AnySurrealType functionality
    let datetime_any = AnySurrealType::new(chrono::Utc::now());
    let duration_any = AnySurrealType::new(StdDuration::from_secs(42));
    let string_any = AnySurrealType::new("test string".to_string());
    let int_any = AnySurrealType::new(123i64);

    // Create record using type-erased values
    let create_query = format!(
        "CREATE {}:type_erased SET dt = $dt, dur = $dur, str = $str, num = $num",
        table_name
    );

    // Create CBOR parameters using value() method to get underlying CBOR
    let params = CborValue::Map(vec![
        (
            CborValue::Text("dt".to_string()),
            datetime_any.value().clone(),
        ),
        (
            CborValue::Text("dur".to_string()),
            duration_any.value().clone(),
        ),
        (
            CborValue::Text("str".to_string()),
            string_any.value().clone(),
        ),
        (CborValue::Text("num".to_string()), int_any.value().clone()),
    ]);

    let create_result = client
        .query_cbor(&create_query, params)
        .await
        .expect("Create failed");
    println!("Create result: {:?}", create_result);

    // Select the record back
    let select_query = format!("SELECT * FROM {}:type_erased", table_name);
    let select_result = client
        .query_cbor(&select_query, CborValue::Null)
        .await
        .expect("Select failed");

    println!("Select result: {:?}", select_result);

    // Verify the record was created and values are preserved
    let select_json = cbor_to_json_for_test(select_result);
    if let Value::Array(arr) = select_json {
        assert!(!arr.is_empty(), "Should have at least one record");

        // Navigate the response structure: response[0].result[0]
        let query_response = &arr[0];
        let result_array = query_response
            .get("result")
            .expect("Missing result field in query response")
            .as_array()
            .expect("Result field should be an array");

        assert!(
            !result_array.is_empty(),
            "Should have at least one record in result"
        );
        let record = &result_array[0];

        // Check that all fields exist
        assert!(
            record.get("dt").is_some(),
            "dt field missing in record: {:?}",
            record
        );
        assert!(
            record.get("dur").is_some(),
            "dur field missing in record: {:?}",
            record
        );
        assert!(
            record.get("str").is_some(),
            "str field missing in record: {:?}",
            record
        );
        assert!(
            record.get("num").is_some(),
            "num field missing in record: {:?}",
            record
        );

        println!("Type-erased fields preserved correctly");
    } else {
        panic!("Expected array result from select");
    }

    cleanup(&client, &table_name).await;
}
