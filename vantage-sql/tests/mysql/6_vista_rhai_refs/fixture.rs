//! Suffixed `client` and `orders` tables, the specs whose references script
//! their targets, and the helpers the tests assert with.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql::vista::MysqlVistaSpec;
use vantage_types::Record;
use vantage_vista::SpecResolver;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// The default `orders` script: alice's orders above 15, so only `o2`
/// matches. The `total` condition proves the script ran, not the FK path.
pub(super) const ORDERS_SCRIPT: &str =
    r#"table("orders").where("client_id", me.id).where("total", "gt", 15)"#;

pub(super) struct Fixture {
    pub(super) db: MysqlDB,
    client_t: String,
    pub(super) orders_t: String,
}

impl Fixture {
    pub(super) async fn new(suffix: &str) -> Self {
        let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
        let fixture = Self {
            db,
            client_t: format!("vista_rref_client_{suffix}"),
            orders_t: format!("vista_rref_orders_{suffix}"),
        };
        fixture.drop_tables().await;
        let (client_t, orders_t) = (&fixture.client_t, &fixture.orders_t);
        for sql in [
            format!("CREATE TABLE `{client_t}` (id VARCHAR(255) PRIMARY KEY, name TEXT NOT NULL)"),
            format!(
                "CREATE TABLE `{orders_t}` (id VARCHAR(255) PRIMARY KEY, \
                 client_id VARCHAR(255) NOT NULL, total BIGINT NOT NULL)"
            ),
            format!("INSERT INTO `{client_t}` VALUES ('alice','Alice'),('bob','Bob')"),
            format!(
                "INSERT INTO `{orders_t}` VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
            ),
        ] {
            sqlx::query(&sql).execute(fixture.db.pool()).await.unwrap();
        }
        fixture
    }

    pub(super) async fn drop_tables(&self) {
        for t in [&self.client_t, &self.orders_t] {
            sqlx::query(&format!("DROP TABLE IF EXISTS `{t}`"))
                .execute(self.db.pool())
                .await
                .unwrap();
        }
    }

    /// A `client` spec whose `orders` reference evaluates `script`.
    pub(super) fn client_spec(&self, script: &str) -> MysqlVistaSpec {
        let client_t = &self.client_t;
        spec(&format!(
            r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
mysql:
  table: {client_t}
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
    mysql:
      rhai: |
        {script}
"#
        ))
    }

    /// A read-only, query-sourced view over the client table with the same
    /// scripted `orders` reference.
    pub(super) fn client_view_spec(&self) -> MysqlVistaSpec {
        let client_t = &self.client_t;
        spec(&format!(
            r#"
name: client_view
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
mysql:
  rhai: |
    sql::select().from("{client_t}").field("id").field("name")
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
    mysql:
      rhai: |
        {ORDERS_SCRIPT}
"#
        ))
    }

    pub(super) fn orders_spec(&self) -> MysqlVistaSpec {
        let orders_t = &self.orders_t;
        spec(&format!(
            r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
mysql:
  table: {orders_t}
"#
        ))
    }

    /// A `base: client_base` vista that declares its own scripted `orders`
    /// reference; the base itself has no references.
    pub(super) fn derived_client_spec(&self) -> MysqlVistaSpec {
        spec(&format!(
            r#"
name: client_derived
columns: {{}}
mysql:
  base: client_base
  inherit:
    columns: [id, name]
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
    mysql:
      rhai: |
        {ORDERS_SCRIPT}
"#
        ))
    }

    fn client_base_spec(&self) -> MysqlVistaSpec {
        let client_t = &self.client_t;
        spec(&format!(
            r#"
name: client_base
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
mysql:
  table: {client_t}
"#
        ))
    }

    pub(super) fn resolver(&self) -> Arc<dyn SpecResolver<MysqlVistaSpec>> {
        let map: IndexMap<String, MysqlVistaSpec> = [
            ("orders".to_string(), self.orders_spec()),
            ("client_base".to_string(), self.client_base_spec()),
        ]
        .into_iter()
        .collect();
        let map = Arc::new(map);
        Arc::new(move |name: &str| map.get(name).cloned())
    }
}

fn spec(yaml: &str) -> MysqlVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("valid spec")
}

pub(super) fn row_with_id(id: &str) -> Record<CborValue> {
    let mut row = Record::new();
    row.insert("id".to_string(), CborValue::Text(id.into()));
    row
}

pub(super) fn sorted_ids(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    ids
}
