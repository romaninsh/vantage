use std::sync::OnceLock;

use anyhow::Ok;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_with::serde_as;
use uuid::Uuid;
use vantage::{
    prelude::{Entity, Postgres, WritableDataSet},
    sql::Table,
};

use anyhow::Result;

static POSTGRESS: OnceLock<Postgres> = OnceLock::new();

pub async fn postgres() -> Postgres {
    if let Some(p) = POSTGRESS.get() {
        return p.clone();
    }

    let connection_string = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@localhost:5432/postgres".to_string());

    let postgres = Postgres::new(&connection_string).await;

    POSTGRESS.set(postgres.clone()).unwrap();

    postgres
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct TestStruct {
    small_int: i16,
    integer: i32,
    big_int: i64,
    float4: f32,
    float8: f64,
    boolean: bool,
    date: NaiveDate,            // For "2024-03-14"
    time: NaiveTime,            // For "13:14:15"
    timestamp: NaiveDateTime,   // For "2024-03-14T13:14:15"
    timestamptz: DateTime<Utc>, // For "2024-03-14T11:14:15+00:00"
    complex_interval: String, // Keep as string since no existing interval type handles this directly
    #[serde_as(as = "serde_with::DurationSeconds<String>")]
    hour_interval: Duration, // Use chrono::Duration
    #[serde_as(as = "serde_with::DurationSeconds<String>")]
    fractional_interval: Duration, // Use chrono::Duration
    #[serde(with = "uuid::serde::simple")]
    uuid: Uuid, // For UUIDs
    json: Value,              // Generic JSON
    jsonb: Value,             // Generic JSON
}
impl Entity for TestStruct {}

#[tokio::test]
#[ignore]
async fn my_test() -> Result<()> {
    let p = postgres();

    let t = Table::new_with_entity("test_types", p.await)
        .with_column("small_int")
        .with_id_column("integer")
        .with_column("big_int")
        .with_column("float4")
        .with_column("float8")
        .with_column("boolean")
        .with_column("date")
        .with_column("time")
        .with_column("timestamp")
        .with_column("timestamptz")
        .with_column("complex_interval")
        .with_column("hour_interval")
        .with_column("fractional_interval")
        .with_column("uuid")
        .with_column("json")
        .with_column("jsonb");

    let max_values = TestStruct {
        small_int: i16::MAX,
        integer: i32::MAX,
        big_int: i64::MAX,
        float4: f32::MAX,
        float8: f64::MAX,
        boolean: true,
        date: NaiveDate::from_ymd_opt(9999, 12, 31).unwrap(),
        time: NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
        timestamp: Utc
            .timestamp_opt(253402300799, 999_999_999)
            .unwrap()
            .naive_utc(),
        timestamptz: Utc.timestamp_opt(253402300799, 999_999_999).unwrap(),
        complex_interval: "999 years 12 mons 31 days".to_string(),
        hour_interval: Duration::hours(3000),
        fractional_interval: Duration::days(3000),
        uuid: Uuid::nil(), // Replace with meaningful maximum UUID if needed
        json: json!({"max_key": "max_value"}),
        jsonb: json!({"max_key": "max_value"}),
    };

    let min_values = TestStruct {
        small_int: i16::MIN,
        integer: i32::MIN,
        big_int: i64::MIN,
        float4: f32::MIN,
        float8: f64::MIN,
        boolean: false,
        date: NaiveDate::from_ymd_opt(1, 1, 1).unwrap(),
        time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        timestamp: Utc.timestamp_opt(-62135596800, 0).unwrap().naive_utc(),
        timestamptz: Utc.timestamp_opt(-62135596800, 0).unwrap(),
        complex_interval: "0 years 0 mons 0 days".to_string(),
        hour_interval: Duration::zero(),
        fractional_interval: Duration::zero(),
        uuid: Uuid::nil(),
        json: json!({"min_key": "min_value"}),
        jsonb: json!({"min_key": "min_value"}),
    };

    t.insert(min_values).await?;

    Ok(())
}
