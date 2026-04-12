//! Chrono type round-trip via Table<SqliteDB, Entity>.
//!
//! SQLite stores all date/time types as TEXT. Tests verify that chrono
//! types survive the full entity → insert → read → entity pipeline.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
#[allow(unused_imports)]
use vantage_sql::sqlite::AnySqliteType;
use vantage_sql::sqlite::SqliteDB;
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Event {
    name: String,
    event_date: NaiveDate,
    start_time: NaiveTime,
    created_at: NaiveDateTime,
    published_at: DateTime<Utc>,
}

impl Event {
    fn table(db: SqliteDB) -> Table<SqliteDB, Event> {
        Table::new("event", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<NaiveDate>("event_date")
            .with_column_of::<NaiveTime>("start_time")
            .with_column_of::<NaiveDateTime>("created_at")
            .with_column_of::<DateTime<Utc>>("published_at")
    }
}

fn test_event() -> Event {
    Event {
        name: "Launch Party".to_string(),
        event_date: NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
        start_time: NaiveTime::from_hms_opt(14, 30, 0).unwrap(),
        created_at: NaiveDateTime::parse_from_str("2025-01-10 09:00:00", "%Y-%m-%d %H:%M:%S")
            .unwrap(),
        published_at: "2025-01-10T12:00:00Z".parse().unwrap(),
    }
}

async fn setup() -> Table<SqliteDB, Event> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE event (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            event_date TEXT NOT NULL,
            start_time TEXT NOT NULL,
            created_at TEXT NOT NULL,
            published_at TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    Event::table(db)
}

#[tokio::test]
async fn test_chrono_text_columns() {
    let table = setup().await;
    let original = test_event();

    let inserted = table.insert(&"evt1".to_string(), &original).await.unwrap();
    assert_eq!(inserted, original);

    let fetched = table.get("evt1").await.unwrap();
    assert_eq!(fetched, original);
}

// ═════════════════════════════════════════════════════════════════════════
// Subsecond precision — TEXT columns preserve microseconds
// ═════════════════════════════════════════════════════════════════════════

// ═════════════════════════════════════════════════════════════════════════
// DateTime<FixedOffset> — TEXT preserves timezone offset
// ═════════════════════════════════════════════════════════════════════════

use chrono::FixedOffset;

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct EventFixed {
    name: String,
    value: DateTime<FixedOffset>,
}

impl EventFixed {
    fn table(db: SqliteDB) -> Table<SqliteDB, EventFixed> {
        Table::new("event_fixed", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<DateTime<FixedOffset>>("value")
    }
}

async fn setup_fixed() -> Table<SqliteDB, EventFixed> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE event_fixed (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            value TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    EventFixed::table(db)
}

#[tokio::test]
async fn test_chrono_text_fixed_offset() {
    let table = setup_fixed().await;
    let offset = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
    let original = EventFixed {
        name: "india".to_string(),
        value: NaiveDate::from_ymd_opt(2025, 1, 10)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap()
            .and_local_timezone(offset)
            .unwrap(),
    };

    let inserted = table.insert(&"fix1".to_string(), &original).await.unwrap();
    assert_eq!(inserted, original);

    let fetched = table.get("fix1").await.unwrap();
    assert_eq!(fetched, original);
    // Offset preserved as +05:30
    assert_eq!(fetched.value.offset().local_minus_utc(), 5 * 3600 + 30 * 60);
}

// ═════════════════════════════════════════════════════════════════════════
// Subsecond precision — TEXT columns preserve microseconds
// ═════════════════════════════════════════════════════════════════════════

fn test_event_subsec() -> Event {
    Event {
        name: "Subsecond Party".to_string(),
        event_date: NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
        start_time: NaiveTime::from_hms_micro_opt(14, 30, 0, 123456).unwrap(),
        created_at: NaiveDate::from_ymd_opt(2025, 1, 10)
            .unwrap()
            .and_hms_micro_opt(9, 0, 0, 123456)
            .unwrap(),
        published_at: NaiveDate::from_ymd_opt(2025, 1, 10)
            .unwrap()
            .and_hms_micro_opt(12, 0, 0, 123456)
            .unwrap()
            .and_utc(),
    }
}

#[tokio::test]
async fn test_chrono_text_subsec() {
    let table = setup().await;
    let original = test_event_subsec();

    let inserted = table.insert(&"evt_sub".to_string(), &original).await.unwrap();
    assert_eq!(inserted, original);

    let fetched = table.get("evt_sub").await.unwrap();
    assert_eq!(fetched, original);
}
