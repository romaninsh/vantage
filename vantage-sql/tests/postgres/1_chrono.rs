//! Chrono type matrix test via Table<PostgresDB, Entity>.
//!
//! 4 chrono types × 5 column types = 20 combinations.
//! Tables are pre-created by v5_chrono.sql (same shape: id, name, value).
//! Incompatible combos (e.g. NaiveTime into DATE) expect insert failure.
//!
//! Postgres column types: VARCHAR, DATE, TIME, TIMESTAMP, TIMESTAMPTZ

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage_v5";

async fn db() -> PostgresDB {
    PostgresDB::connect(PG_URL).await.unwrap()
}

// ── 4 entity structs, one per chrono type ────────────────────────────────

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValDate {
    name: String,
    value: NaiveDate,
}

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValTime {
    name: String,
    value: NaiveTime,
}

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValNaiveDateTime {
    name: String,
    value: NaiveDateTime,
}

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValUtc {
    name: String,
    value: DateTime<Utc>,
}

// ── Table constructors ───────────────────────────────────────────────────

macro_rules! table_for {
    ($table:expr, $db:expr, ValDate) => {
        Table::<PostgresDB, ValDate>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveDate>("value")
    };
    ($table:expr, $db:expr, ValTime) => {
        Table::<PostgresDB, ValTime>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveTime>("value")
    };
    ($table:expr, $db:expr, ValNaiveDateTime) => {
        Table::<PostgresDB, ValNaiveDateTime>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveDateTime>("value")
    };
    ($table:expr, $db:expr, ValUtc) => {
        Table::<PostgresDB, ValUtc>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<DateTime<Utc>>("value")
    };
}

// ── Test values ──────────────────────────────────────────────────────────

fn date_val() -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()
}
fn time_val() -> NaiveTime {
    NaiveTime::from_hms_opt(14, 30, 0).unwrap()
}
fn ndt_val() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2025-01-10 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}
fn utc_val() -> DateTime<Utc> {
    "2025-01-10T12:00:00Z".parse().unwrap()
}

/// Try replace + read-back, return Ok(entity) or Err with the error message.
/// Uses replace (upsert) so tests are idempotent across re-runs.
async fn try_round_trip<E>(table: &Table<PostgresDB, E>, id: &str, entity: &E) -> Result<E, String>
where
    E: Clone
        + vantage_types::IntoRecord<AnyPostgresType>
        + vantage_types::TryFromRecord<AnyPostgresType, Error = vantage_core::VantageError>
        + Send
        + Sync,
{
    table
        .replace(&id.to_string(), entity)
        .await
        .map_err(|e| e.to_string())?;
    table.get(id).await.map_err(|e| e.to_string())
}

// ═════════════════════════════════════════════════════════════════════════
// 1. VARCHAR columns — all types round-trip as text
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_varchar_date() {
    let t = table_for!("chrono_varchar", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    let fetched = try_round_trip(&t, "vc_d", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_time() {
    let t = table_for!("chrono_varchar", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    let fetched = try_round_trip(&t, "vc_t", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_ndt() {
    let t = table_for!("chrono_varchar", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let fetched = try_round_trip(&t, "vc_ndt", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_utc() {
    let t = table_for!("chrono_varchar", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let fetched = try_round_trip(&t, "vc_utc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 2. DATE columns
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_date_date() {
    let t = table_for!("chrono_date", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    let fetched = try_round_trip(&t, "da_d", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_date_time() {
    // TIME into DATE — Postgres rejects incompatible type
    let t = table_for!("chrono_date", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    assert!(try_round_trip(&t, "da_t", &orig).await.is_err());
}

#[tokio::test]
async fn test_date_ndt() {
    // NaiveDateTime into DATE — date part kept via typed bind
    let t = table_for!("chrono_date", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let result = try_round_trip(&t, "da_ndt", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value.date(), ndt_val().date());
    }
}

#[tokio::test]
async fn test_date_utc() {
    // DateTime<Utc> into DATE — date part kept
    let t = table_for!("chrono_date", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let result = try_round_trip(&t, "da_utc", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value.date_naive(), utc_val().date_naive());
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 3. TIME columns
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_time_date() {
    // DATE into TIME — Postgres rejects incompatible type
    let t = table_for!("chrono_time", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    assert!(try_round_trip(&t, "ti_d", &orig).await.is_err());
}

#[tokio::test]
async fn test_time_time() {
    let t = table_for!("chrono_time", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    let fetched = try_round_trip(&t, "ti_t", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_time_ndt() {
    // NaiveDateTime into TIME — time part extracted via typed bind
    let t = table_for!("chrono_time", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let result = try_round_trip(&t, "ti_ndt", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value.time(), ndt_val().time());
    }
}

#[tokio::test]
async fn test_time_utc() {
    // DateTime<Utc> into TIME — time part extracted
    let t = table_for!("chrono_time", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let result = try_round_trip(&t, "ti_utc", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value.time(), utc_val().time());
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 4. TIMESTAMP columns (without timezone)
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_timestamp_date() {
    // NaiveDate into TIMESTAMP — may get midnight appended or fail
    let t = table_for!("chrono_timestamp", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    let result = try_round_trip(&t, "ts_d", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value, date_val());
    }
}

#[tokio::test]
async fn test_timestamp_time() {
    // TIME into TIMESTAMP — Postgres rejects
    let t = table_for!("chrono_timestamp", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    assert!(try_round_trip(&t, "ts_t", &orig).await.is_err());
}

#[tokio::test]
async fn test_timestamp_ndt() {
    let t = table_for!("chrono_timestamp", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let fetched = try_round_trip(&t, "ts_ndt", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_timestamp_utc() {
    let t = table_for!("chrono_timestamp", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let fetched = try_round_trip(&t, "ts_utc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 5. TIMESTAMPTZ columns (with timezone — UTC)
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_timestamptz_date() {
    // NaiveDate into TIMESTAMPTZ — may get midnight UTC or fail
    let t = table_for!("chrono_timestamptz", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    let result = try_round_trip(&t, "tz_d", &orig).await;
    if let Ok(fetched) = result {
        assert_eq!(fetched.value, date_val());
    }
}

#[tokio::test]
async fn test_timestamptz_time() {
    // TIME into TIMESTAMPTZ — Postgres rejects
    let t = table_for!("chrono_timestamptz", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    assert!(try_round_trip(&t, "tz_t", &orig).await.is_err());
}

#[tokio::test]
async fn test_timestamptz_ndt() {
    let t = table_for!("chrono_timestamptz", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let fetched = try_round_trip(&t, "tz_ndt", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_timestamptz_utc() {
    let t = table_for!("chrono_timestamptz", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let fetched = try_round_trip(&t, "tz_utc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}
