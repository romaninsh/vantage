use surreal_client::{SurrealClient, SurrealConnection};
use vantage_expressions::ExprDataSource;
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
                let Value::Map(_first_record) = result_array.get(0).expect("Expected first record") else {
                    panic!("Expected record map")
                };
            }
        }
    };
}

macro_type_test!("strings", {
    s1: "hello",
    s2: String::from("world")
});

macro_type_test!("booleans", {
    flag: true
});

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
    f32_val: 3.14f32,
    f64_val: 2.718281828f64
});

macro_type_test!("decimals", {
    decimal_val: {
        "92233720368547758.123456789012345678".parse::<rust_decimal::Decimal>().unwrap()
    }
});

macro_type_test!("optionals", {
    some_int: Some(42i32),
    none_int: None::<i32>,
    some_str: Some("hello"),
    none_str: None::<&str>
});

macro_type_test!("json_values", {
    json_string: serde_json::json!("hello"),
    json_number: serde_json::json!(42),
    json_object: serde_json::json!({"name": "test", "value": 123}),
    json_array: serde_json::json!([1, 2, 3])
});

#[tokio::test]
async fn test_missmatching_types() {
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
    let Value::Map(first_record) = result_array.get(0).expect("Expected first record") else {
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
