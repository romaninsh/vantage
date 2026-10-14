//! `mysql: { modify }` scripts and the `with_condition` verb: the script runs
//! against the built vista as `self` and narrows it with SQL expressions from
//! the MySQL Rhai vocabulary.
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
use vantage_vista::{Capability, NativeCondition, SpecResolver, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

struct Fixture {
    db: MysqlDB,
    orders_t: String,
}

impl Fixture {
    async fn new(suffix: &str) -> Self {
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

    async fn drop_tables(&self) {
        sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", self.orders_t))
            .execute(self.db.pool())
            .await
            .unwrap();
    }

    /// The `orders` spec with `mysql:` block lines appended (each indented
    /// two spaces under `mysql:`).
    fn orders_spec(&self, mysql_block: &str) -> MysqlVistaSpec {
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

    fn plain_orders_spec(&self) -> MysqlVistaSpec {
        self.orders_spec(&format!("  table: {}", self.orders_t))
    }

    fn resolver(&self) -> Arc<dyn SpecResolver<MysqlVistaSpec>> {
        let map: IndexMap<String, MysqlVistaSpec> =
            [("orders".to_string(), self.plain_orders_spec())]
                .into_iter()
                .collect();
        let map = Arc::new(map);
        Arc::new(move |name: &str| map.get(name).cloned())
    }
}

fn spec(yaml: &str) -> MysqlVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("valid spec")
}

fn sorted_ids(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn modify_script_adds_sql_condition() -> TestResult {
    let f = Fixture::new("cond").await;
    let block = format!(
        "  table: {}\n  modify: |\n    self.with_condition(sql::ident(\"total\") > 15)",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2", "o3"]);
    assert!(
        orders.capabilities().has(Capability::Insert),
        "a modified table stays writable"
    );
    // The script's condition is recorded by its rendered SQL.
    assert_eq!(orders.query().native.len(), 1);
    assert!(orders.query().native[0].contains("total"));

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_composes_with_rhai_source() -> TestResult {
    let f = Fixture::new("rhai").await;
    let block = format!(
        "  rhai: |\n    sql::select().from(\"{}\").field(\"id\").field(\"client_id\").field(\"total\")\n  \
         modify: |\n    self.with_condition(sql::ident(\"client_id\") == \"alice\")",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;
    assert!(
        !orders.capabilities().has(Capability::Insert),
        "query-sourced vista is read-only"
    );
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1", "o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_composes_with_base() -> TestResult {
    let f = Fixture::new("base").await;
    let derived = spec(
        r#"
name: big_orders
columns:
  id: { type: string, flags: [id] }
  total: { type: int }
mysql:
  base: orders
  inherit:
    columns: [id, total]
  modify: |
    self.with_condition(sql::ident("total") >= 20)
"#,
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let big = factory.build_from_spec(derived)?;
    assert_eq!(sorted_ids(&big.list_values().await?), vec!["o2", "o3"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_table_resolves_through_resolver() -> TestResult {
    let f = Fixture::new("table").await;
    let block = format!(
        "  table: {}\n  modify: |\n    table(\"orders\").where(\"client_id\", \"bob\")",
        f.orders_t
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let orders = factory.build_from_spec(f.orders_spec(&block))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o3"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn native_condition_rejects_foreign_type() -> TestResult {
    let f = Fixture::new("raw").await;
    let mut orders =
        f.db.vista_factory()
            .build_from_spec(f.plain_orders_spec())?;
    let err = orders
        .add_native_condition(NativeCondition::new(42_i32, "42"))
        .expect_err("an i32 is not a MySQL condition");
    assert!(err.is_incorrect_usage(), "a wrong type is a caller mistake");
    assert!(err.to_string().contains("native condition"), "{err}");
    assert!(
        orders.query().native.is_empty(),
        "a refused condition isn't recorded"
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn native_condition_accepts_mysql_expression() -> TestResult {
    let f = Fixture::new("rawok").await;
    let mut orders =
        f.db.vista_factory()
            .build_from_spec(f.plain_orders_spec())?;
    let expr = vantage_sql::mysql_expr!("total < {}", 15_i64);
    let key = expr.preview();
    orders.add_native_condition(NativeCondition::new(expr, key.clone()))?;
    assert_eq!(orders.query().native, vec![key]);
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1"]);

    f.drop_tables().await;
    Ok(())
}

/// `with_condition` only narrows `self`; on a `table(...)` handle it is
/// refused, as on every backend.
#[tokio::test]
async fn scripted_ref_refuses_with_condition_on_table_handle() -> TestResult {
    let f = Fixture::new("refcond").await;
    let client = spec(
        r#"
name: client
columns:
  id: { type: string, flags: [id] }
mysql:
  table: unused_client
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
    mysql:
      rhai: |
        table("orders").with_condition(sql::ident("total") > 15)
"#,
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(client)?;
    let mut row = Record::new();
    row.insert("id".to_string(), CborValue::Text("alice".into()));
    let err = clients
        .get_ref("orders", Some(&row))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );

    f.drop_tables().await;
    Ok(())
}

fn order(client_id: &str, total: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("client_id".to_string(), CborValue::Text(client_id.into()));
    record.insert("total".to_string(), CborValue::Integer(total.into()));
    record
}

async fn stored_orders(f: &Fixture) -> Vec<(String, i64)> {
    sqlx::query_as::<_, (String, i64)>(&format!(
        "SELECT id, total FROM `{}` ORDER BY id",
        f.orders_t
    ))
    .fetch_all(f.db.pool())
    .await
    .unwrap()
}

/// Writes through a `modify:`-narrowed vista keep the narrowing: a row held
/// outside the condition, or a new row that does not fit it, is a `Conflict`
/// and nothing is written.
#[tokio::test]
async fn modify_script_condition_guards_writes() -> TestResult {
    let f = Fixture::new("guard").await;
    sqlx::query(&format!("DELETE FROM `{}`", f.orders_t))
        .execute(f.db.pool())
        .await?;
    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('o1','alice',10)",
        f.orders_t
    ))
    .execute(f.db.pool())
    .await?;
    let before = stored_orders(&f).await;

    let block = format!(
        "  table: {}\n  modify: |\n    self.with_condition(sql::ident(\"total\") > 15)",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;

    let held: IndexMap<String, Record<CborValue>> = [("o1".to_string(), order("alice", 20))]
        .into_iter()
        .collect();
    let Err(err) = orders.import_values(&held).await else {
        panic!("an id held outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let misfit: IndexMap<String, Record<CborValue>> = [("o2".to_string(), order("alice", 5))]
        .into_iter()
        .collect();
    let Err(err) = orders.import_values(&misfit).await else {
        panic!("a row outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let Err(err) = orders
        .insert_value("o3".to_string(), &order("alice", 5))
        .await
    else {
        panic!("an insert outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let fits: IndexMap<String, Record<CborValue>> = [("o4".to_string(), order("alice", 20))]
        .into_iter()
        .collect();
    assert_eq!(orders.import_values(&fits).await?, 1);
    assert_eq!(
        stored_orders(&f).await,
        vec![("o1".to_string(), 10), ("o4".to_string(), 20)]
    );

    f.drop_tables().await;
    Ok(())
}
