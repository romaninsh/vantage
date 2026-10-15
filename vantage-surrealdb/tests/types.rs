use chrono::{DateTime, Utc};
use ciborium::Value as CborValue;
use surreal_client::{SurrealClient, SurrealConnection};
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::{SurrealType, surrealdb::SurrealDB, types::AnySurrealType};

const DB_URL: &str = "cbor://localhost:8000/rpc";
const ROOT_USER: &str = "root";
const ROOT_PASS: &str = "root";
const TEST_NAMESPACE: &str = "bakery";
const TEST_DATABASE: &str = "types";

async fn get_client() -> SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .with_debug(true)
        .connect()
        .await
        .expect("Failed to connect to SurrealDB")
}

async fn get_surrealdb() -> SurrealDB {
    let client = get_client().await;
    SurrealDB::new(client)
}

macro_rules! macro_type_test {
    ($table:literal, { $($field:ident: $value:expr),* }) => {
        paste::paste! {
            #[tokio::test]
            async fn [<test_ $table _types>]() {
                let db = get_surrealdb().await;

                // Clean up any existing test data first
                let cleanup_expr = vantage_expressions::Expression::<AnySurrealType>::new(
                    format!("DELETE {}", $table),
                    vec![],
                );
                let _ = db.execute(&cleanup_expr).await;

                let create_expr = vantage_expressions::Expression::<AnySurrealType>::new(
                    format!("CREATE {}:unique_record SET {}", $table,
                        vec![$(concat!(stringify!($field), "={}")),*].join(", ")
                    ),
                    vec![
                        $(vantage_expressions::ExpressiveEnum::Scalar(AnySurrealType::new($value))),*
                    ],
                );

                let _create_result = db
                    .execute(&create_expr)
                    .await
                    .expect("Failed to execute create query");

                let select_expr = vantage_expressions::Expression::<AnySurrealType>::new(
                    format!("SELECT {} FROM {}:unique_record",
                        vec![$(stringify!($field)),*].join(", "),
                        $table
                    ),
                    vec![],
                );

                let select_result = db
                    .execute(&select_expr)
                    .await
                    .expect("Failed to execute select query");

                use ciborium::Value;
                let Value::Array(result_array) = select_result.value() else {
                    panic!("Expected result array")
                };
                let Value::Map(_first_record) = result_array.first().expect("Expected first record") else {
                    panic!("Expected record map")
                };
            }
        }
    };
}

macro_type_test!("numbers", {
    i8_val: 42i8,
    i16_val: 1234i16,
    i32_val: 123456i32,
    i64_val: 123456789i64,
    isize_val: 987654isize,
    u8_val: 255u8,
    u16_val: 65535u16,
    u32_val: 4294967295u32,
    u64_val: 18446744073709551615u64,
    usize_val: 123456usize,
    f32_val: 3.15f32,
    f64_val: 2.719281828f64
});

#[tokio::test]
async fn test_optionals_store_none_and_values() {
    use ciborium::Value;
    use indexmap::IndexMap;

    let db = get_surrealdb().await;
    let run = |query: &'static str, args: Vec<AnySurrealType>| {
        let expr = vantage_expressions::Expression::<AnySurrealType>::new(
            query,
            args.into_iter()
                .map(vantage_expressions::ExpressiveEnum::Scalar)
                .collect(),
        );
        let db = db.clone();
        async move { db.execute(&expr).await }
    };

    // The table may not exist yet; a failed cleanup is fine.
    let _ = run("DELETE optionals_stored", vec![]).await;
    run(
        "CREATE optionals_stored:unique_record SET some_int={}, none_int={}, some_str={}, none_str={}",
        vec![
            AnySurrealType::new(Some(42i32)),
            AnySurrealType::new(None::<i32>),
            AnySurrealType::new(Some("hello")),
            AnySurrealType::new(None::<&str>),
        ],
    )
    .await
    .expect("Failed to execute create query");

    let selected = run(
        "SELECT some_int, none_int, some_str, none_str FROM optionals_stored:unique_record",
        vec![],
    )
    .await
    .expect("Failed to execute select query");
    let Value::Array(rows) = selected.value() else {
        panic!("Expected result array")
    };
    let Some(Value::Map(record)) = rows.first() else {
        panic!("Expected record map")
    };
    let record = IndexMap::<String, AnySurrealType>::from_cbor(Value::Map(record.clone()))
        .expect("Should convert to IndexMap");

    assert_eq!(record["some_int"].try_get::<i32>(), Some(42));
    assert_eq!(record["some_str"].try_get::<String>(), Some("hello".into()));
    // SurrealDB drops NONE-valued fields from the stored record; a stored NULL
    // would still be present and read back as `Null`.
    for field in ["none_int", "none_str"] {
        match record.get(field) {
            None => {}
            Some(any) => assert!(
                matches!(any.value(), Value::Null | Value::Tag(6, _)),
                "{field} should be NONE/NULL, got {:?}",
                any.value()
            ),
        }
    }

    let _ = run("DELETE optionals_stored", vec![]).await;
}

#[tokio::test]
async fn test_duration_variant_and_json() {
    let db = get_surrealdb().await;
    let expr = vantage_expressions::Expression::<AnySurrealType>::new("RETURN 1h30m", vec![]);
    let result = db.execute(&expr).await.expect("Failed to execute query");

    assert_eq!(
        result.type_variant(),
        Some(vantage_surrealdb::SurrealTypeVariants::Duration),
        "unexpected CBOR: {:?}",
        result.value()
    );
    let json: serde_json::Value = result.into();
    assert_eq!(json, serde_json::Value::String("PT1H30M".to_string()));
}

/// Tagged SurrealDB values convert to JSON in the text SurrealDB's own JSON
/// API gives: a datetime as RFC 3339, a record id as `table:id`.
#[tokio::test]
async fn tagged_values_convert_to_surreal_json_text() {
    let db = get_surrealdb().await;
    for (query, expected) in [
        (
            "RETURN <datetime> '2025-01-01T00:00:00Z'",
            "2025-01-01T00:00:00Z",
        ),
        ("RETURN user:alice", "user:alice"),
        ("RETURN user:42", "user:42"),
    ] {
        let expr = vantage_expressions::Expression::<AnySurrealType>::new(query, vec![]);
        let json: serde_json::Value = db.execute(&expr).await.expect(query).into();
        assert_eq!(json, serde_json::Value::String(expected.into()), "{query}");
    }

    let rfc3339 = AnySurrealType::from(CborValue::Tag(
        0,
        Box::new(CborValue::Text("2025-01-01T00:00:00Z".into())),
    ));
    let json: serde_json::Value = rfc3339.into();
    assert_eq!(
        json,
        serde_json::Value::String("2025-01-01T00:00:00Z".into())
    );
}

/// A record id held as `table:id` text decodes as a `Thing`.
#[test]
fn record_id_text_decodes_as_a_thing() {
    assert_eq!(
        Thing::from_cbor(CborValue::Text("user:alice".into())),
        Some(Thing::new("user", "alice"))
    );
}

/// Datetimes also decode from RFC 3339 text, plain or tagged, so records that
/// store them as strings still load; other text is no datetime.
#[test]
fn datetime_decodes_from_rfc3339_text() {
    let expected: DateTime<Utc> = "2026-03-19T20:25:38Z".parse().unwrap();
    let text = || CborValue::Text("2026-03-19T20:25:38Z".into());
    assert_eq!(DateTime::<Utc>::from_cbor(text()), Some(expected));
    assert_eq!(
        DateTime::<Utc>::from_cbor(CborValue::Tag(0, Box::new(text()))),
        Some(expected)
    );
    assert_eq!(
        DateTime::<Utc>::from_cbor(CborValue::Text("not a date".into())),
        None
    );
}

/// An array displays as a SurrealQL array literal and a map as an object
/// literal, each item as it displays on its own.
#[test]
fn arrays_and_maps_display_as_surrealql_literals() {
    let tags = AnySurrealType::new(vec!["rust".to_string(), "db".to_string()]);
    assert_eq!(tags.to_string(), r#"["rust", "db"]"#);
    assert_eq!(AnySurrealType::new(vec![1i64, 2]).to_string(), "[1, 2]");

    let map: indexmap::IndexMap<String, AnySurrealType> = [
        ("a".to_string(), AnySurrealType::new(1i64)),
        ("b".to_string(), AnySurrealType::new("x".to_string())),
    ]
    .into_iter()
    .collect();
    assert_eq!(AnySurrealType::new(map).to_string(), r#"{a: 1, b: "x"}"#);
}

#[tokio::test]
async fn test_mismatching_types() {
    let db = get_surrealdb().await;

    let s1_value = "hello";
    let s2_value = "world".to_string();
    let s3_value = 123;

    // Clean up any existing test data first
    let cleanup_expr =
        vantage_expressions::Expression::<AnySurrealType>::new("DELETE mismatching_types", vec![]);
    let _ = db.execute(&cleanup_expr).await;

    let create_expr = vantage_expressions::Expression::<AnySurrealType>::new(
        "create mismatching_types:unique_record set s1={}, s2={}, s3={}",
        vec![
            vantage_expressions::ExpressiveEnum::Scalar(AnySurrealType::new(s1_value)),
            vantage_expressions::ExpressiveEnum::Scalar(AnySurrealType::new(s2_value)),
            vantage_expressions::ExpressiveEnum::Scalar(AnySurrealType::new(s3_value)),
        ],
    );

    let _ = db
        .execute(&create_expr)
        .await
        .expect("Failed to execute create query");

    let select_expr = vantage_expressions::Expression::<AnySurrealType>::new(
        "SELECT s1, s2, s3 FROM mismatching_types:unique_record",
        vec![],
    );
    let select_result = db
        .execute(&select_expr)
        .await
        .expect("Failed to execute select query");

    use ciborium::Value;
    let Value::Array(result_array) = select_result.value() else {
        panic!("Expected result array")
    };
    let Value::Map(first_record) = result_array.first().expect("Expected first record") else {
        panic!("Expected record map")
    };

    // Convert CBOR map to IndexMap<String, AnySurrealType> using existing SurrealType implementation
    use indexmap::IndexMap;
    let cbor_map = Value::Map(first_record.clone());
    let record_map = IndexMap::<String, AnySurrealType>::from_cbor(cbor_map)
        .expect("Should convert to IndexMap");

    // Try to convert all 3 types to strings using try_get
    let s1_as_string = record_map.get("s1").unwrap().try_get::<String>();
    let s2_as_string = record_map.get("s2").unwrap().try_get::<String>();
    let s3_as_string = record_map.get("s3").unwrap().try_get::<String>();

    // s1 and s2 should convert to strings, s3 (number) should return None
    assert_eq!(s1_as_string, Some("hello".to_string()));
    assert_eq!(s2_as_string, Some("world".to_string()));
    assert_eq!(s3_as_string, None);
}

// #[tokio::test]
// async fn test_bool_types() {
//     let client = get_client().await;

//     // Test boolean type variants in the booleans table
//     let test_cases = vec![("bool1", true), ("bool2", false)];

//     for (record_id, test_bool) in test_cases {
//         // Test type conversion
//         let any_bool = AnySurrealType::new(test_bool);
//         let cbor_value = any_bool.value();
//         assert!(matches!(cbor_value, ciborium::Value::Bool(_)));

//         let restored = <bool as SurrealType>::from_cbor(cbor_value.clone()).unwrap();
//         assert_eq!(test_bool, restored);

//         // Test database round-trip in booleans table
//         let create_query = format!("CREATE booleans:{} SET value = $value", record_id);
//         let params = serde_json::json!({ "value": test_bool });

//         client
//             .query(&create_query, Some(params))
//             .await
//             .expect("Failed to create bool record");

//         // Verify retrieval
//         let select_query = format!("SELECT * FROM booleans:{}", record_id);
//         let select_result = client
//             .query(&select_query, None)
//             .await
//             .expect("Failed to select bool record");

//         if let JsonValue::Array(arr) = select_result {
//             if let Some(JsonValue::Array(result_array)) = arr.get(0) {
//                 if let Some(record) = result_array.get(0) {
//                     let stored_value = record.get("value").expect("Missing value field");
//                     assert_eq!(stored_value, &JsonValue::Bool(test_bool));
//                 }
//             }
//         }

//         // Clean up
//         let delete_query = format!("DELETE booleans:{}", record_id);
//         client.query(&delete_query, None).await.ok();
//     }

//     println!("✅ Bool types test passed");
// }

// #[tokio::test]
// async fn test_number_types() {
//     let client = get_client().await;

//     // Test all numeric type variants in the numbers table
//     let record_id = "num1";
//     let create_query = format!(
//         "CREATE numbers:{} SET
//          i8_val = $i8_val,
//          i16_val = $i16_val,
//          i32_val = $i32_val,
//          i64_val = $i64_val,
//          isize_val = $isize_val,
//          u8_val = $u8_val,
//          u16_val = $u16_val,
//          u32_val = $u32_val,
//          u64_val = $u64_val,
//          usize_val = $usize_val,
//          f32_val = $f32_val,
//          f64_val = $f64_val",
//         record_id
//     );

//     let params = serde_json::json!({
//         "i8_val": 42i8,
//         "i16_val": 1234i16,
//         "i32_val": 123456i32,
//         "i64_val": 123456789i64,
//         "isize_val": 987654,
//         "u8_val": 255u8,
//         "u16_val": 65535u16,
//         "u32_val": 4294967295u32,
//         "u64_val": 18446744073709551615u64,
//         "usize_val": 123456,
//         "f32_val": 3.14f32,
//         "f64_val": 2.718281828f64
//     });

//     // Test type conversions for each numeric type
//     let any_i32 = AnySurrealType::new(42i32);
//     let cbor_i32 = any_i32.value();
//     assert!(matches!(cbor_i32, ciborium::Value::Integer(_)));
//     let restored_i32 = <i32 as SurrealType>::from_cbor(cbor_i32.clone()).unwrap();
//     assert_eq!(42i32, restored_i32);

//     let any_f64 = AnySurrealType::new(3.14f64);
//     let cbor_f64 = any_f64.value();
//     assert!(matches!(cbor_f64, ciborium::Value::Float(_)));
//     let restored_f64 = <f64 as SurrealType>::from_cbor(cbor_f64.clone()).unwrap();
//     assert!((3.14f64 - restored_f64).abs() < f64::EPSILON);

//     client
//         .query(&create_query, Some(params))
//         .await
//         .expect("Failed to create numbers record");

//     // Verify retrieval
//     let select_query = format!("SELECT * FROM numbers:{}", record_id);
//     let select_result = client
//         .query(&select_query, None)
//         .await
//         .expect("Failed to select numbers record");

//     if let JsonValue::Array(arr) = select_result {
//         if let Some(JsonValue::Array(result_array)) = arr.get(0) {
//             if let Some(record) = result_array.get(0) {
//                 // Verify some key numeric fields
//                 if let JsonValue::Number(i32_num) = record.get("i32_val").unwrap() {
//                     assert_eq!(i32_num.as_i64().unwrap(), 123456);
//                 }
//                 if let JsonValue::Number(f64_num) = record.get("f64_val").unwrap() {
//                     let stored_float = f64_num.as_f64().unwrap();
//                     assert!((2.718281828 - stored_float).abs() < 0.000001);
//                 }
//             }
//         }
//     }

//     // Clean up
//     let delete_query = format!("DELETE numbers:{}", record_id);
//     client.query(&delete_query, None).await.ok();

//     println!("✅ Number types test passed");
// }

// #[cfg(feature = "rust_decimal")]
// #[tokio::test]
// async fn test_decimal_types() {
//     let client = get_client().await;

//     // Test decimal type variants in the decimals table (using rust_decimal feature)
//     let _record_id = "dec1";

//     // Test basic decimal values as floats (since we removed custom Decimal)
//     let test_cases = vec![
//         ("dec1", 123.456f64),
//         ("dec2", -987.654321f64),
//         ("dec3", 0.000000001f64),
//     ];

//     for (rid, decimal_val) in test_cases {
//         let create_query = format!("CREATE decimals:{} SET value = $value", rid);
//         let params = serde_json::json!({ "value": decimal_val });

//         client
//             .query(&create_query, Some(params))
//             .await
//             .expect("Failed to create decimal record");

//         // Verify retrieval
//         let select_query = format!("SELECT * FROM decimals:{}", rid);
//         let select_result = client
//             .query(&select_query, None)
//             .await
//             .expect("Failed to select decimal record");

//         if let JsonValue::Array(arr) = select_result {
//             if let Some(JsonValue::Array(result_array)) = arr.get(0) {
//                 if let Some(record) = result_array.get(0) {
//                     let stored_value = record.get("value").expect("Missing value field");
//                     if let JsonValue::Number(num) = stored_value {
//                         let stored_decimal = num.as_f64().unwrap();
//                         assert!((decimal_val - stored_decimal).abs() < 0.000001);
//                     }
//                 }
//             }
//         }

//         // Clean up
//         let delete_query = format!("DELETE decimals:{}", rid);
//         client.query(&delete_query, None).await.ok();
//     }

//     println!("✅ Decimal types test passed");
// }

// #[cfg(not(feature = "rust_decimal"))]
// #[tokio::test]
// async fn test_decimal_types() {
//     println!("✅ Decimal types test skipped (rust_decimal feature not enabled)");
// }
