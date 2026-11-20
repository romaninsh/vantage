//! `vantage_vista::contract::check_temporal` over MySQL, on a vista built from YAML with declared
//! `date`, `time` and `timestamp` columns.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_sql::mysql::MysqlDB;
use vantage_types::temporal::{TAG_DATE, TAG_INSTANT, TAG_TIME};
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

struct Fixture {
    db: MysqlDB,
    tables: AtomicUsize,
}

impl Fixture {
    async fn new() -> Self {
        Self {
            db: MysqlDB::connect(MYSQL_URL).await.unwrap(),
            tables: AtomicUsize::new(0),
        }
    }

    async fn execute(&self, sql: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(self.db.pool())
            .await
            .map_err(|e| error!(format!("{sql}: {e}")))?;
        Ok(())
    }
}

#[async_trait]
impl TemporalFixture for Fixture {
    async fn fresh(&self) -> Vista {
        let name = format!("temporal_{}", self.tables.fetch_add(1, Ordering::SeqCst));
        for sql in [
            format!("DROP TABLE IF EXISTS `{name}`"),
            format!(
                "CREATE TABLE `{name}` (
                    id VARCHAR(64) PRIMARY KEY,
                    d DATE NULL,
                    t TIME(6) NULL,
                    ts DATETIME(6) NULL,
                    tz TIMESTAMP(6) NULL
                )"
            ),
        ] {
            self.execute(&sql).await.unwrap();
        }
        self.db
            .vista_factory()
            .from_yaml(&format!(
                "name: {name}\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  \
                 d: {{ type: date }}\n  t: {{ type: time }}\n  ts: {{ type: timestamp }}\n  \
                 tz: {{ type: timestamp }}\nmysql:\n  table: {name}\n"
            ))
            .unwrap()
    }

    fn zone_aware(&self) -> bool {
        true
    }

    /// `TIMESTAMP` holds 1970-01-01 to 2038-01-19 only.
    fn zone_aware_full_range(&self) -> bool {
        false
    }

    async fn stored(&self, vista: &Vista, id: &str, column: &str) -> Result<Stored> {
        let (kind, render) = match column {
            "d" => ("DATE", "DATE_FORMAT(d, '%Y-%m-%d')"),
            "t" => ("TIME(6)", "TIME_FORMAT(t, '%H:%i:%s.%f')"),
            "ts" => ("DATETIME(6)", "DATE_FORMAT(ts, '%Y-%m-%dT%H:%i:%s.%fZ')"),
            "tz" => (
                "TIMESTAMP(6)",
                "DATE_FORMAT(CONVERT_TZ(tz, @@session.time_zone, '+00:00'), '%Y-%m-%dT%H:%i:%s.%fZ')",
            ),
            other => return Err(error!(format!("the table has no temporal column {other}"))),
        };
        let sql = format!("SELECT {render} FROM `{}` WHERE id = ?", vista.name());
        let canonical: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(id)
            .fetch_one(self.db.pool())
            .await
            .map_err(|e| error!(format!("{sql}: {e}")))?;
        Ok(match canonical {
            Some(canonical) => Stored::Native {
                kind: kind.to_string(),
                canonical,
            },
            None => Stored::Carrier(CborValue::Null),
        })
    }

    async fn seed_foreign(&self, vista: &Vista) -> Result<Vec<Foreign>> {
        let tag = |n: u64, s: &str| CborValue::Tag(n, Box::new(CborValue::Text(s.to_string())));
        let foreign = [
            (
                "ts",
                "'2026-11-19 16:01:00'",
                tag(TAG_INSTANT, "2026-11-19T16:01:00.000000Z"),
            ),
            (
                "ts",
                "'2026-11-19 16:01:00.123456'",
                tag(TAG_INSTANT, "2026-11-19T16:01:00.123456Z"),
            ),
            ("d", "'2026-11-19'", tag(TAG_DATE, "2026-11-19")),
            ("t", "'16:01'", tag(TAG_TIME, "16:01:00.000000")),
        ];
        let mut seeded = Vec::new();
        for (i, (column, literal, reads_as)) in foreign.into_iter().enumerate() {
            self.execute(&format!(
                "INSERT INTO `{}` (id, {column}) VALUES ('f{i}', {literal})",
                vista.name()
            ))
            .await?;
            seeded.push(Foreign::new(
                column,
                format!("{literal} in {column}"),
                reads_as,
            ));
        }
        Ok(seeded)
    }
}

#[tokio::test]
async fn mysql_temporal_contract() {
    check_temporal(&Fixture::new().await).await;
}
