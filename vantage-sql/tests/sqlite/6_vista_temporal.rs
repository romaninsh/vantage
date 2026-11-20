//! `vantage_vista::contract::check_temporal` over in-memory SQLite, through a YAML vista with
//! declared `date` / `time` / `timestamp` columns on plain `TEXT` storage.

#![cfg(feature = "vista")]

use std::sync::Mutex;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use sqlx::AssertSqlSafe;
use vantage_core::{Result, error};
use vantage_sql::sqlite::SqliteDB;
use vantage_types::temporal::{TAG_DATE, TAG_INSTANT, TAG_TIME};
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

const YAML: &str = r#"
name: event
columns:
  id: { type: string, flags: [id] }
  d: { type: date }
  t: { type: time }
  ts: { type: timestamp }
sqlite:
  table: event
"#;

/// Keeps the last table's database, so `stored` and `seed_foreign` reach past the vista.
struct Fixture {
    db: Mutex<Option<SqliteDB>>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            db: Mutex::new(None),
        }
    }

    fn db(&self) -> SqliteDB {
        self.db.lock().unwrap().clone().expect("fresh() ran first")
    }
}

fn tagged(tag: u64, s: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(s.to_string())))
}

#[async_trait]
impl TemporalFixture for Fixture {
    async fn fresh(&self) -> Vista {
        let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE event (id TEXT PRIMARY KEY, d TEXT, t TEXT, ts TEXT)")
            .execute(db.pool())
            .await
            .unwrap();
        *self.db.lock().unwrap() = Some(db.clone());
        db.vista_factory().from_yaml(YAML).expect("loads")
    }

    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> Result<Stored> {
        let sql = format!("SELECT {column} FROM event WHERE id = ?");
        let raw: Option<String> = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(id)
            .fetch_one(self.db().pool())
            .await
            .map_err(|e| error!(format!("reading {column} of {id}: {e}")))?;
        raw.map(Stored::Text)
            .ok_or_else(|| error!(format!("{column} of {id} is NULL")))
    }

    async fn seed_foreign(&self, _vista: &Vista) -> Result<Vec<Foreign>> {
        let at = tagged(TAG_INSTANT, "2026-11-19T16:01:00.000000Z");
        let half = tagged(TAG_INSTANT, "2026-11-19T16:01:00.500000Z");
        let day = tagged(TAG_DATE, "2026-11-19");
        let time = tagged(TAG_TIME, "16:01:00.000000");
        let foreign = [
            ("ts", "2026-11-19T16:01:00Z", at.clone()),
            ("ts", "2026-11-19 16:01:00", at.clone()),
            ("ts", "2026-11-19 16:01:00.5", half),
            ("ts", "2026-11-19T17:01:00+01:00", at),
            ("d", "2026-11-19", day),
            ("t", "16:01", time.clone()),
            ("t", "16:01:00", time),
        ];
        let db = self.db();
        let mut seeded = Vec::new();
        for (i, (column, written, reads_as)) in foreign.into_iter().enumerate() {
            let sql = format!("INSERT INTO event (id, {column}) VALUES (?, ?)");
            sqlx::query(AssertSqlSafe(sql))
                .bind(format!("f{i}"))
                .bind(written)
                .execute(db.pool())
                .await
                .map_err(|e| error!(format!("seeding {column} = {written}: {e}")))?;
            seeded.push(Foreign::new(column, format!("{written:?}"), reads_as));
        }
        Ok(seeded)
    }
}

#[tokio::test]
async fn sqlite_temporal_contract() {
    check_temporal(&Fixture::new()).await;
}
