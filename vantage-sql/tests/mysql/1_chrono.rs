//! Chrono type matrix test via Table<MysqlDB, Entity>.
//!
//! 4 chrono types × 5 column types = 20 combinations.
//! Tables are pre-created by v5_chrono.sql (same shape: id, name, value).
//! Incompatible combos (e.g. NaiveTime into DATE) expect insert failure.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage_v5";

async fn db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL).await.unwrap()
}

// ── 4 entity structs, one per chrono type ────────────────────────────────

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValDate {
    name: String,
    value: NaiveDate,
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValTime {
    name: String,
    value: NaiveTime,
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValNaiveDateTime {
    name: String,
    value: NaiveDateTime,
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValUtc {
    name: String,
    value: DateTime<Utc>,
}

// ── Table constructors ───────────────────────────────────────────────────

macro_rules! table_for {
    ($table:expr, $db:expr, ValDate) => {
        Table::<MysqlDB, ValDate>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveDate>("value")
    };
    ($table:expr, $db:expr, ValTime) => {
        Table::<MysqlDB, ValTime>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveTime>("value")
    };
    ($table:expr, $db:expr, ValNaiveDateTime) => {
        Table::<MysqlDB, ValNaiveDateTime>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveDateTime>("value")
    };
    ($table:expr, $db:expr, ValUtc) => {
        Table::<MysqlDB, ValUtc>::new($table, $db)
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
async fn try_round_trip<E>(table: &Table<MysqlDB, E>, id: &str, entity: &E) -> Result<E, String>
where
    E: Clone
        + vantage_types::IntoRecord<AnyMysqlType>
        + vantage_types::TryFromRecord<AnyMysqlType, Error = vantage_core::VantageError>
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
    // TIME into DATE column — MySQL rejects
    let t = table_for!("chrono_date", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    assert!(try_round_trip(&t, "da_t", &orig).await.is_err());
}

#[tokio::test]
async fn test_date_ndt() {
    // NaiveDateTime into DATE — insert works but entity read fails:
    // DATE column returns Date variant, NaiveDateTime expects DateTime variant
    let t = table_for!("chrono_date", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    assert!(try_round_trip(&t, "da_ndt", &orig).await.is_err());
}

#[tokio::test]
async fn test_date_utc() {
    // DateTime<Utc> into DATE — same: Date variant vs DateTime target
    let t = table_for!("chrono_date", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    assert!(try_round_trip(&t, "da_utc", &orig).await.is_err());
}

// ═════════════════════════════════════════════════════════════════════════
// 3. TIME columns
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_time_date() {
    // DATE into TIME column — MySQL rejects
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
    // NaiveDateTime into TIME — Time variant vs DateTime target
    let t = table_for!("chrono_time", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    assert!(try_round_trip(&t, "ti_ndt", &orig).await.is_err());
}

#[tokio::test]
async fn test_time_utc() {
    // DateTime<Utc> into TIME — Time variant vs DateTime target
    let t = table_for!("chrono_time", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    assert!(try_round_trip(&t, "ti_utc", &orig).await.is_err());
}

// ═════════════════════════════════════════════════════════════════════════
// 4. DATETIME columns
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_datetime_date() {
    // NaiveDate into DATETIME — gets midnight appended
    let t = table_for!("chrono_datetime", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    // NaiveDate into DATETIME — DateTime variant returned, NaiveDate can't
    // parse "2025-06-15 00:00:00" with %Y-%m-%d alone
    assert!(try_round_trip(&t, "dt_d", &orig).await.is_err());
}

#[tokio::test]
async fn test_datetime_time() {
    // TIME into DATETIME — MySQL rejects
    let t = table_for!("chrono_datetime", db().await, ValTime);
    let orig = ValTime {
        name: "t".into(),
        value: time_val(),
    };
    assert!(try_round_trip(&t, "dt_t", &orig).await.is_err());
}

#[tokio::test]
async fn test_datetime_ndt() {
    let t = table_for!("chrono_datetime", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt".into(),
        value: ndt_val(),
    };
    let fetched = try_round_trip(&t, "dt_ndt", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_datetime_utc() {
    // DateTime<Utc> into DATETIME — timezone stripped
    let t = table_for!("chrono_datetime", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc".into(),
        value: utc_val(),
    };
    let fetched = try_round_trip(&t, "dt_utc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 5. TIMESTAMP columns — UTC-aware
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_timestamp_date() {
    // NaiveDate into TIMESTAMP — gets midnight UTC
    let t = table_for!("chrono_timestamp", db().await, ValDate);
    let orig = ValDate {
        name: "d".into(),
        value: date_val(),
    };
    // Same as datetime_date — NaiveDate can't parse datetime string from Tag(0)
    assert!(try_round_trip(&t, "ts_d", &orig).await.is_err());
}

#[tokio::test]
async fn test_timestamp_time() {
    // TIME into TIMESTAMP — MySQL rejects
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
// 6. Subsecond precision — TIME/DATETIME/TIMESTAMP(0) truncate, (6) preserve
// ═════════════════════════════════════════════════════════════════════════

fn time_subsec() -> NaiveTime {
    NaiveTime::from_hms_micro_opt(14, 30, 0, 123456).unwrap()
}
fn ndt_subsec() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2025, 1, 10)
        .unwrap()
        .and_hms_micro_opt(9, 0, 0, 123456)
        .unwrap()
}
fn ndt_subsec_truncated() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2025, 1, 10)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()
}
fn utc_subsec() -> DateTime<Utc> {
    ndt_subsec().and_utc()
}
fn utc_subsec_truncated() -> DateTime<Utc> {
    ndt_subsec_truncated().and_utc()
}

// ── VARCHAR — subseconds preserved as text ──────────────────────────────

#[tokio::test]
async fn test_varchar_time_subsec() {
    let t = table_for!("chrono_varchar", db().await, ValTime);
    let orig = ValTime {
        name: "t_sub".into(),
        value: time_subsec(),
    };
    let fetched = try_round_trip(&t, "vc_t_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_ndt_subsec() {
    let t = table_for!("chrono_varchar", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt_sub".into(),
        value: ndt_subsec(),
    };
    let fetched = try_round_trip(&t, "vc_ndt_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_utc_subsec() {
    let t = table_for!("chrono_varchar", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc_sub".into(),
        value: utc_subsec(),
    };
    let fetched = try_round_trip(&t, "vc_utc_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ── TIME(0) — subseconds truncated ─────────────────────────────────────

#[tokio::test]
async fn test_time0_time_subsec() {
    let t = table_for!("chrono_time", db().await, ValTime);
    let orig = ValTime {
        name: "t_sub".into(),
        value: time_subsec(),
    };
    let fetched = try_round_trip(&t, "ti_t_sub", &orig).await.unwrap();
    // Subseconds truncated — only whole seconds survive
    assert_ne!(fetched.value, time_subsec());
    assert_eq!(fetched.value, NaiveTime::from_hms_opt(14, 30, 0).unwrap());
}

// ── TIME(6) — microseconds preserved ───────────────────────────────────

#[tokio::test]
async fn test_time6_time_subsec() {
    let t = table_for!("chrono_time6", db().await, ValTime);
    let orig = ValTime {
        name: "t_sub".into(),
        value: time_subsec(),
    };
    let fetched = try_round_trip(&t, "ti6_t_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ── DATETIME(0) — subseconds truncated ─────────────────────────────────

#[tokio::test]
async fn test_datetime0_ndt_subsec() {
    let t = table_for!("chrono_datetime", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt_sub".into(),
        value: ndt_subsec(),
    };
    let fetched = try_round_trip(&t, "dt_ndt_sub", &orig).await.unwrap();
    assert_ne!(fetched.value, ndt_subsec());
    assert_eq!(fetched.value, ndt_subsec_truncated());
}

#[tokio::test]
async fn test_datetime0_utc_subsec() {
    let t = table_for!("chrono_datetime", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc_sub".into(),
        value: utc_subsec(),
    };
    let fetched = try_round_trip(&t, "dt_utc_sub", &orig).await.unwrap();
    assert_ne!(fetched.value, utc_subsec());
    assert_eq!(fetched.value, utc_subsec_truncated());
}

// ── DATETIME(6) — microseconds preserved ───────────────────────────────

#[tokio::test]
async fn test_datetime6_ndt_subsec() {
    let t = table_for!("chrono_datetime6", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt_sub".into(),
        value: ndt_subsec(),
    };
    let fetched = try_round_trip(&t, "dt6_ndt_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_datetime6_utc_subsec() {
    let t = table_for!("chrono_datetime6", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc_sub".into(),
        value: utc_subsec(),
    };
    let fetched = try_round_trip(&t, "dt6_utc_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ── TIMESTAMP(0) — subseconds truncated ────────────────────────────────

#[tokio::test]
async fn test_timestamp0_ndt_subsec() {
    let t = table_for!("chrono_timestamp", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt_sub".into(),
        value: ndt_subsec(),
    };
    let fetched = try_round_trip(&t, "ts_ndt_sub", &orig).await.unwrap();
    assert_ne!(fetched.value, ndt_subsec());
    assert_eq!(fetched.value, ndt_subsec_truncated());
}

#[tokio::test]
async fn test_timestamp0_utc_subsec() {
    let t = table_for!("chrono_timestamp", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc_sub".into(),
        value: utc_subsec(),
    };
    let fetched = try_round_trip(&t, "ts_utc_sub", &orig).await.unwrap();
    assert_ne!(fetched.value, utc_subsec());
    assert_eq!(fetched.value, utc_subsec_truncated());
}

// ── TIMESTAMP(6) — microseconds preserved ──────────────────────────────

#[tokio::test]
async fn test_timestamp6_ndt_subsec() {
    let t = table_for!("chrono_timestamp6", db().await, ValNaiveDateTime);
    let orig = ValNaiveDateTime {
        name: "ndt_sub".into(),
        value: ndt_subsec(),
    };
    let fetched = try_round_trip(&t, "ts6_ndt_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_timestamp6_utc_subsec() {
    let t = table_for!("chrono_timestamp6", db().await, ValUtc);
    let orig = ValUtc {
        name: "utc_sub".into(),
        value: utc_subsec(),
    };
    let fetched = try_round_trip(&t, "ts6_utc_sub", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 7. DateTime<FixedOffset> — timezone offset as a runtime value
// ═════════════════════════════════════════════════════════════════════════

use chrono::FixedOffset;

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValFixed {
    name: String,
    value: DateTime<FixedOffset>,
}

macro_rules! table_for_fixed {
    ($table:expr, $db:expr) => {
        Table::<MysqlDB, ValFixed>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<DateTime<FixedOffset>>("value")
    };
}

fn fixed_val() -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
    NaiveDate::from_ymd_opt(2025, 1, 10)
        .unwrap()
        .and_hms_opt(14, 30, 0)
        .unwrap()
        .and_local_timezone(offset)
        .unwrap()
}

// ── VARCHAR — offset preserved exactly ──────────────────────────────────

#[tokio::test]
async fn test_varchar_fixed() {
    let t = table_for_fixed!("chrono_varchar", db().await);
    let orig = ValFixed {
        name: "india".into(),
        value: fixed_val(),
    };
    let fetched = try_round_trip(&t, "vc_fix", &orig).await.unwrap();
    assert_eq!(fetched, orig);
    // Offset preserved as +05:30
    assert_eq!(fetched.value.offset().local_minus_utc(), 5 * 3600 + 30 * 60);
}

// ── DATETIME — offset lost, converted to UTC wall clock ────────────────

#[tokio::test]
async fn test_datetime_fixed() {
    let t = table_for_fixed!("chrono_datetime", db().await);
    let orig = ValFixed {
        name: "india".into(),
        value: fixed_val(),
    };
    let fetched = try_round_trip(&t, "dt_fix", &orig).await.unwrap();
    // Same instant but offset normalized to +00:00 (MySQL drops tz)
    assert_eq!(fetched.value, orig.value.with_timezone(&Utc).fixed_offset());
    assert_eq!(fetched.value.offset().local_minus_utc(), 0);
}

// ── TIMESTAMP — offset lost, converted to UTC ──────────────────────────

#[tokio::test]
async fn test_timestamp_fixed() {
    let t = table_for_fixed!("chrono_timestamp", db().await);
    let orig = ValFixed {
        name: "india".into(),
        value: fixed_val(),
    };
    let fetched = try_round_trip(&t, "ts_fix", &orig).await.unwrap();
    // Same instant but offset normalized to +00:00
    assert_eq!(fetched.value, orig.value.with_timezone(&Utc).fixed_offset());
    assert_eq!(fetched.value.offset().local_minus_utc(), 0);
}
