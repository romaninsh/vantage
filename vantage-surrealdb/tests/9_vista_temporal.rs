//! `vantage_vista::contract::check_temporal` over a live SurrealDB 3
//! (`SURREALDB_URL`, defaults to localhost:8000). The fixture works in a
//! randomised database of its own; every `fresh` call defines a numbered
//! table there.

#![cfg(feature = "vista")]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use surreal_client::SurrealConnection;
use uuid::Uuid;
use vantage_core::{Result, error};
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_types::temporal::{TAG_DATE, TAG_INSTANT, TAG_TIME};
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

const INSTANT_FORMAT: &str = "%Y-%m-%dT%H:%M:%S.%6fZ";

struct SurrealTemporal {
    db: SurrealDB,
    next: AtomicUsize,
    /// The table of the last `fresh` vista, so `stored` and `seed_foreign`
    /// reach past it.
    table: Mutex<String>,
}

impl SurrealTemporal {
    async fn new() -> Self {
        let database = format!("vista_temporal_{}", Uuid::new_v4().simple());
        let client = SurrealConnection::dsn(surreal_dsn(&database))
            .expect("Invalid SURREALDB_URL DSN")
            .connect()
            .await
            .expect("Failed to connect to SurrealDB");
        Self {
            db: SurrealDB::new(client),
            next: AtomicUsize::new(0),
            table: Mutex::new(String::new()),
        }
    }

    fn table(&self) -> String {
        let table = self.table.lock().unwrap().clone();
        assert!(!table.is_empty(), "fresh() ran first");
        table
    }
}

#[async_trait]
impl TemporalFixture for SurrealTemporal {
    async fn fresh(&self) -> Vista {
        let table = format!("event_{}", self.next.fetch_add(1, Ordering::SeqCst));
        let define = format!("DEFINE TABLE {table} SCHEMALESS");
        self.db
            .execute(&surreal_expr!(define.as_str()))
            .await
            .unwrap();
        *self.table.lock().unwrap() = table.clone();
        self.db
            .vista_factory()
            .from_yaml(&format!(
                r#"
name: {table}
columns:
  id: {{ type: thing, flags: [id, orderable] }}
  d: {{ type: date, flags: [orderable] }}
  t: {{ type: time, flags: [orderable] }}
  ts: {{ type: timestamp, flags: [orderable] }}
"#
            ))
            .unwrap()
    }

    /// A datetime is rendered by SurrealQL: `d` as its day when it falls on
    /// midnight UTC, anything else as the instant's UTC wall clock.
    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> Result<Stored> {
        let table = self.table();
        let c = column;
        let rendered = if column == "d" {
            format!(
                "IF time::floor({c}, 1d) = {c} THEN time::format({c}, '%Y-%m-%d') \
                 ELSE time::format({c}, '{INSTANT_FORMAT}') END"
            )
        } else {
            format!("time::format({c}, '{INSTANT_FORMAT}')")
        };
        let query = format!(
            "SELECT VALUE \
             IF {c} = NONE THEN ['none', ''] \
             ELSE IF type::is_datetime({c}) THEN ['datetime', {rendered}] \
             ELSE IF type::is_string({c}) THEN ['string', {c}] \
             ELSE ['other', <string> {c}] END \
             FROM ONLY {table}:{id}"
        );
        let pair: Vec<String> = self
            .db
            .execute(&surreal_expr!(query.as_str()))
            .await?
            .try_get()
            .ok_or_else(|| error!(format!("{table}:{id}.{column} is not readable as a pair")))?;
        let [kind, value]: [String; 2] = pair
            .try_into()
            .map_err(|p| error!(format!("{table}:{id}.{column} reads {p:?}")))?;
        Ok(match kind.as_str() {
            "datetime" => Stored::Native {
                kind,
                canonical: value,
            },
            "string" => Stored::Text(value),
            other => Stored::Text(format!("<{other}> {value}")),
        })
    }

    async fn seed_foreign(&self, _vista: &Vista) -> Result<Vec<Foreign>> {
        let table = self.table();
        let tag = |n: u64, s: &str| CborValue::Tag(n, Box::new(CborValue::Text(s.to_string())));
        let instant = tag(TAG_INSTANT, "2026-11-19T16:01:00.000000Z");
        let day = tag(TAG_DATE, "2026-11-19");
        let foreign = [
            ("ts", r#"d"2026-11-19T16:01:00Z""#, instant.clone()),
            ("ts", r#""2026-11-19T16:01:00Z""#, instant),
            ("d", r#"d"2026-11-19T00:00:00Z""#, day.clone()),
            ("d", r#""2026-11-19""#, day),
            ("t", r#""16:01""#, tag(TAG_TIME, "16:01:00.000000")),
        ];
        let mut seeded = Vec::new();
        for (i, (column, written, reads_as)) in foreign.into_iter().enumerate() {
            let create = format!("CREATE {table}:f{i} SET {column} = {written}");
            self.db.execute(&surreal_expr!(create.as_str())).await?;
            seeded.push(Foreign::new(column, written, reads_as));
        }
        Ok(seeded)
    }
}

#[tokio::test]
async fn surrealdb_temporal_contract() {
    check_temporal(&SurrealTemporal::new().await).await;
}
