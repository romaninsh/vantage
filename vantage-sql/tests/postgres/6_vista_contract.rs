//! `vantage_vista::contract` over PostgreSQL.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Each test uses its own suffixed tables to avoid collisions.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_types::temporal::{TAG_DATE, TAG_INSTANT, TAG_TIME};
use vantage_vista::contract::{self, Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

/// Clients `alice` and `bob`; orders `o1`, `o2` (alice) and `o3` (bob), in
/// tables named `contract_client_<suffix>` and `contract_orders_<suffix>`.
async fn setup(suffix: &str) -> PostgresDB {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let (client, orders) = names(suffix);
    for sql in [
        format!("DROP TABLE IF EXISTS \"{client}\""),
        format!("DROP TABLE IF EXISTS \"{orders}\""),
        format!("CREATE TABLE \"{client}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL)"),
        format!("CREATE TABLE \"{orders}\" (id TEXT PRIMARY KEY, client_id TEXT, total BIGINT)"),
        format!("INSERT INTO \"{client}\" VALUES ('alice','Alice'),('bob','Bob')"),
        format!(
            "INSERT INTO \"{orders}\" VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(db.pool())
            .await
            .unwrap();
    }
    db
}

fn names(suffix: &str) -> (String, String) {
    (
        format!("contract_client_{suffix}"),
        format!("contract_orders_{suffix}"),
    )
}

fn orders(db: PostgresDB, suffix: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(&names(suffix).1, db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<i64>("total")
}

fn clients(db: PostgresDB, suffix: &str) -> Table<PostgresDB, EmptyEntity> {
    let target = db.clone();
    let suffix_owned = suffix.to_string();
    Table::<PostgresDB, EmptyEntity>::new(&names(suffix).0, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", move |_| {
            orders(target.clone(), &suffix_owned)
        })
}

#[tokio::test]
async fn postgres_contract_advertised() {
    let db = setup("advertised").await;
    let factory = db.vista_factory();
    let client = factory
        .from_table(clients(db.clone(), "advertised"))
        .unwrap();
    contract::check_advertised(&client).await;
    let orders = factory
        .from_table(orders(db.clone(), "advertised"))
        .unwrap();
    contract::check_advertised_writes(&orders).await;
}

#[tokio::test]
async fn postgres_contract_clone() {
    let db = setup("clone").await;
    let vista = db
        .vista_factory()
        .from_table(orders(db.clone(), "clone"))
        .unwrap();
    contract::check_clone(&vista, "client_id", CborValue::Text("alice".into())).await;
}

/// Each `fresh()` takes the next `contract_temporal_<n>` table, dropped and recreated, so
/// a run leaves at most one table per probe behind.
struct Temporal {
    db: PostgresDB,
    next: AtomicUsize,
}

impl Temporal {
    async fn new() -> Self {
        Self {
            db: PostgresDB::connect(PG_URL).await.unwrap(),
            next: AtomicUsize::new(0),
        }
    }

    async fn exec(&self, sql: String) -> vantage_core::Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(self.db.pool())
            .await
            .map_err(|e| {
                error!(
                    "postgres statement failed",
                    sql = sql.clone(),
                    detail = e.to_string()
                )
            })?;
        Ok(())
    }
}

/// `column` rendered by Postgres itself in the carrier's text form; a naive timestamp is a UTC
/// wall clock by convention.
fn canonical_sql(column: &str) -> Option<&'static str> {
    Some(match column {
        "d" => "to_char(d, 'YYYY-MM-DD')",
        "t" => "to_char(DATE '2000-01-01' + t, 'HH24:MI:SS.US')",
        "ts" => "to_char(ts, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
        "tz" => "to_char(tz AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
        _ => return None,
    })
}

fn tagged(tag: u64, s: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(s.into())))
}

#[async_trait]
impl TemporalFixture for Temporal {
    async fn fresh(&self) -> Vista {
        let name = format!(
            "contract_temporal_{}",
            self.next.fetch_add(1, Ordering::SeqCst)
        );
        self.exec(format!("DROP TABLE IF EXISTS \"{name}\""))
            .await
            .unwrap();
        self.exec(format!(
            "CREATE TABLE \"{name}\" (
                id TEXT PRIMARY KEY,
                d DATE,
                t TIME,
                ts TIMESTAMP WITHOUT TIME ZONE,
                tz TIMESTAMPTZ
            )"
        ))
        .await
        .unwrap();
        self.db
            .vista_factory()
            .from_yaml(&format!(
                "name: {name}\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  \
                 d: {{ type: date }}\n  t: {{ type: time }}\n  ts: {{ type: timestamp }}\n  \
                 tz: {{ type: timestamp }}\npostgres:\n  table: {name}\n"
            ))
            .expect("temporal vista loads")
    }

    fn zone_aware(&self) -> bool {
        true
    }

    async fn stored(&self, vista: &Vista, id: &str, column: &str) -> vantage_core::Result<Stored> {
        let render = canonical_sql(column)
            .ok_or_else(|| error!("not a temporal column", column = column.to_string()))?;
        let sql = format!(
            "SELECT pg_typeof({column})::text, {render} FROM \"{}\" WHERE id = $1",
            vista.name()
        );
        let (kind, canonical): (String, Option<String>) =
            sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(id)
                .fetch_one(self.db.pool())
                .await
                .map_err(|e| {
                    error!(
                        "postgres read failed",
                        sql = sql.clone(),
                        detail = e.to_string()
                    )
                })?;
        Ok(Stored::Native {
            kind,
            canonical: canonical.unwrap_or_default(),
        })
    }

    async fn seed_foreign(&self, vista: &Vista) -> vantage_core::Result<Vec<Foreign>> {
        let instant = |s: &str| tagged(TAG_INSTANT, s);
        // As Odoo and psql write them: naive wall clocks, an offset, short times.
        let foreign = [
            (
                "ts",
                "2026-11-19 16:01:00",
                instant("2026-11-19T16:01:00.000000Z"),
            ),
            (
                "ts",
                "2026-11-19 16:01:00.123456",
                instant("2026-11-19T16:01:00.123456Z"),
            ),
            (
                "tz",
                "2026-11-19 17:01:00+01",
                instant("2026-11-19T16:01:00.000000Z"),
            ),
            ("d", "2026-11-19", tagged(TAG_DATE, "2026-11-19")),
            ("t", "16:01", tagged(TAG_TIME, "16:01:00.000000")),
        ];
        let mut seeded = Vec::new();
        for (i, (column, written, reads_as)) in foreign.into_iter().enumerate() {
            self.exec(format!(
                "INSERT INTO \"{}\" (id, {column}) VALUES ('f{i}', '{written}')",
                vista.name()
            ))
            .await?;
            seeded.push(Foreign::new(column, format!("'{written}'"), reads_as));
        }
        Ok(seeded)
    }
}

#[tokio::test]
async fn postgres_temporal_contract() {
    check_temporal(&Temporal::new().await).await;
}
