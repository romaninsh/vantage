//! Scripted references: a YAML reference with a `mysql: { rhai }` block
//! builds its traversal target by evaluating the script, with `table(name)`
//! resolved through the factory's spec resolver and the parent row exposed as
//! `row` and `me`.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.

use std::error::Error;
use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql::vista::MysqlVistaSpec;
use vantage_types::Record;
use vantage_vista::{Capability, SpecResolver, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// The default `orders` script: alice's orders above 15, so only `o2`
/// matches. The `total` condition proves the script ran, not the FK path.
const ORDERS_SCRIPT: &str = r#"table("orders").where("client_id", me.id).where("total", "gt", 15)"#;

struct Fixture {
    db: MysqlDB,
    client_t: String,
    orders_t: String,
}

impl Fixture {
    async fn new(suffix: &str) -> Self {
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

    async fn drop_tables(&self) {
        for t in [&self.client_t, &self.orders_t] {
            sqlx::query(&format!("DROP TABLE IF EXISTS `{t}`"))
                .execute(self.db.pool())
                .await
                .unwrap();
        }
    }

    /// A `client` spec whose `orders` reference evaluates `script`.
    fn client_spec(&self, script: &str) -> MysqlVistaSpec {
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
    fn client_view_spec(&self) -> MysqlVistaSpec {
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

    fn orders_spec(&self) -> MysqlVistaSpec {
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
    fn derived_client_spec(&self) -> MysqlVistaSpec {
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

    fn resolver(&self) -> Arc<dyn SpecResolver<MysqlVistaSpec>> {
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

fn row_with_id(id: &str) -> Record<CborValue> {
    let mut row = Record::new();
    row.insert("id".to_string(), CborValue::Text(id.into()));
    row
}

fn sorted_ids(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn build_script_reference_resolves_target() -> TestResult {
    let f = Fixture::new("resolve").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let reference = clients.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let alice = clients.get_value("alice").await?.expect("alice exists");
    let orders = clients.get_ref("orders", Some(&alice))?;
    assert_eq!(orders.name(), "orders");
    let rows = orders.list_values().await?;
    assert_eq!(sorted_ids(&rows), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

/// The metadata names a scripted reference's target by its spec (`orders`),
/// not by the physical table that spec reads, and carries the script.
#[tokio::test]
async fn build_script_reference_targets_the_spec_name() -> TestResult {
    let f = Fixture::new("target").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let reference = clients.reference("orders").expect("orders ref");
    assert_eq!(reference.target, "orders");
    assert_ne!(reference.target, f.orders_t);
    assert_eq!(
        reference.build_script.as_deref().map(str::trim),
        Some(ORDERS_SCRIPT)
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn build_script_reference_sees_row() -> TestResult {
    let f = Fixture::new("row").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(
        f.client_spec(r#"table("orders").where("client_id", row.id).where("total", "lt", 15)"#),
    )?;

    let orders = clients.get_ref("orders", Some(&row_with_id("alice")))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1"]);
    // The target is the writable table.
    assert!(orders.capabilities().has(Capability::Insert));

    f.drop_tables().await;
    Ok(())
}

/// A scripted reference is still a traversal: both the table and the
/// query-sourced view advertise `Traverse`.
#[tokio::test]
async fn capabilities_advertise_traverse_for_scripted_refs() -> TestResult {
    let f = Fixture::new("caps").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let orders = factory.build_from_spec(f.orders_spec())?;
    assert!(orders.capabilities().has(Capability::Traverse));
    let view = factory.build_from_spec(f.client_view_spec())?;
    assert!(view.capabilities().has(Capability::Traverse));

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn scripted_ref_from_query_sourced_view_row() -> TestResult {
    let f = Fixture::new("view").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let view = factory.build_from_spec(f.client_view_spec())?;
    assert!(
        !view.capabilities().has(Capability::Insert),
        "query-sourced view is read-only"
    );

    let alice = view.get_value("alice").await?.expect("alice in view");
    let orders = view.get_ref("orders", Some(&alice))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn scripted_ref_declared_on_base_derived_vista() -> TestResult {
    let f = Fixture::new("derived").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let derived = factory.build_from_spec(f.derived_client_spec())?;
    let reference = derived.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let alice = derived.get_value("alice").await?.expect("alice in view");
    let orders = derived.get_ref("orders", Some(&alice))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn script_table_unknown_to_resolver_errors() -> TestResult {
    let f = Fixture::new("unknown").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(r#"table("nope")"#))?;
    let err = clients
        .get_ref("orders", Some(&row_with_id("alice")))
        .map(drop)
        .expect_err("an unknown table must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown table") && msg.contains("nope"),
        "{msg}"
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn script_table_without_resolver_errors() -> TestResult {
    let f = Fixture::new("noresolver").await;
    let factory = f.db.vista_factory();
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let err = clients
        .get_ref("orders", Some(&row_with_id("alice")))
        .map(drop)
        .expect_err("table() without a spec resolver must fail");
    assert!(err.to_string().contains("spec resolver"), "{err}");

    f.drop_tables().await;
    Ok(())
}
