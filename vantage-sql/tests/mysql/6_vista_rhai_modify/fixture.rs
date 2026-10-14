//! A suffixed `orders` table, the specs built over it, and the readers the
//! `modify:` tests assert with.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql::vista::MysqlVistaSpec;
use vantage_types::Record;
use vantage_vista::SpecResolver;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

pub(super) struct Fixture {
    pub(super) db: MysqlDB,
    pub(super) orders_t: String,
}

impl Fixture {
    pub(super) async fn new(suffix: &str) -> Self {
        let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
        let fixture = Self {
            db,
            orders_t: format!("vista_modify_orders_{suffix}"),
        };
        fixture.drop_tables().await;
        let orders_t = &fixture.orders_t;
        for sql in [
            format!(
                "CREATE TABLE `{orders_t}` (id VARCHAR(255) PRIMARY KEY, \
                 client_id VARCHAR(255) NOT NULL, total BIGINT NOT NULL)"
            ),
            format!(
                "INSERT INTO `{orders_t}` VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
            ),
        ] {
            sqlx::query(&sql).execute(fixture.db.pool()).await.unwrap();
        }
        fixture
    }

    pub(super) async fn drop_tables(&self) {
        sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", self.orders_t))
            .execute(self.db.pool())
            .await
            .unwrap();
    }

    /// The `orders` spec with `mysql:` block lines appended (each indented
    /// two spaces under `mysql:`).
    pub(super) fn orders_spec(&self, mysql_block: &str) -> MysqlVistaSpec {
        spec(&format!(
            r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
mysql:
{mysql_block}
"#
        ))
    }

    pub(super) fn plain_orders_spec(&self) -> MysqlVistaSpec {
        self.orders_spec(&format!("  table: {}", self.orders_t))
    }

    pub(super) fn resolver(&self) -> Arc<dyn SpecResolver<MysqlVistaSpec>> {
        let map: IndexMap<String, MysqlVistaSpec> =
            [("orders".to_string(), self.plain_orders_spec())]
                .into_iter()
                .collect();
        let map = Arc::new(map);
        Arc::new(move |name: &str| map.get(name).cloned())
    }
}

pub(super) fn spec(yaml: &str) -> MysqlVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("valid spec")
}

pub(super) fn sorted_ids(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    ids
}

pub(super) fn order(client_id: &str, total: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("client_id".to_string(), CborValue::Text(client_id.into()));
    record.insert("total".to_string(), CborValue::Integer(total.into()));
    record
}

pub(super) async fn stored_orders(f: &Fixture) -> Vec<(String, i64)> {
    sqlx::query_as::<_, (String, i64)>(&format!(
        "SELECT id, total FROM `{}` ORDER BY id",
        f.orders_t
    ))
    .fetch_all(f.db.pool())
    .await
    .unwrap()
}
