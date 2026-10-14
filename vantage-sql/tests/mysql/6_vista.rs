//! Vista integration: typed `Table<MysqlDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.
//! Each test uses a uniquely-suffixed table to avoid collisions. The cases
//! every SQL backend shares come from `vista_tests!` (`tests/common/vista.rs`).

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql::vista::MysqlVistaSpec;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, ReferenceKind, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn setup(suffix: &str) -> (MysqlDB, String) {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let table_name = format!("vista_product_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (
            id VARCHAR(255) PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT 0
        )",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES \
         ('a', 'Alpha', 10, 0), \
         ('b', 'Beta', 20, 1), \
         ('c', 'Gamma', 30, 0)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    (db, table_name)
}

fn product_table(db: MysqlDB, name: &str) -> Table<MysqlDB, EmptyEntity> {
    Table::<MysqlDB, EmptyEntity>::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

async fn product(suffix: &str) -> (MysqlDB, Table<MysqlDB, EmptyEntity>) {
    let (db, name) = setup(suffix).await;
    (db.clone(), product_table(db, &name))
}

vista_tests!(MysqlVistaSpec, "mysql");

#[tokio::test]
async fn vista_lists_typed_mysql_as_cbor() -> TestResult {
    let (db, name) = setup("list").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), name);
    assert_eq!(vista.id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup("caps").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Import,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Traverse,
    ] {
        assert!(caps.has(capability), "mysql advertises {capability}");
    }
    assert!(!caps.has(Capability::Subscribe));
    assert!(!caps.has(Capability::Cursor));
    Ok(())
}

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

/// A `cart` whose `items` column is a JSON array (TEXT), surfaced as a
/// contains-many relation — same JSON-blob round-trip as SQLite.
fn cart_table(db: MysqlDB, table: &str) -> Table<MysqlDB, EmptyEntity> {
    Table::<MysqlDB, EmptyEntity>::new(table, db)
        .with_id_column("id")
        .with_column_of::<String>("items")
        .with_contained_many(
            "items",
            "items",
            |db| {
                Table::new("items", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

#[tokio::test]
async fn contained_json_column_round_trips_on_mysql() -> TestResult {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let t = "vista_cart_contained";
    sqlx::query(&format!("DROP TABLE IF EXISTS `{t}`"))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{t}` (id VARCHAR(255) PRIMARY KEY, items TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        r#"INSERT INTO `{t}` VALUES ('c1', '[{{"sku":"a","qty":1}},{{"sku":"b","qty":2}}]')"#
    ))
    .execute(db.pool())
    .await
    .unwrap();

    let vista = db.vista_factory().from_table(cart_table(db.clone(), t))?;
    assert_eq!(
        vista.references()["items"].kind,
        ReferenceKind::ContainsMany
    );

    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 2);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    assert_eq!(items2.list_values().await?.len(), 3);

    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", Some(&cart3))?;
    assert_eq!(
        items3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );

    sqlx::query(&format!("DROP TABLE `{t}`"))
        .execute(db.pool())
        .await
        .ok();
    Ok(())
}

/// Without the `rhai` feature a `modify:` script cannot run, so the build
/// fails instead of silently skipping it.
#[cfg(not(feature = "rhai"))]
#[tokio::test]
async fn vista_yaml_modify_without_rhai_errors() -> TestResult {
    let db = MysqlDB::connect(MYSQL_URL).await?;
    let yaml = r#"
name: product
columns:
  id: { type: string, flags: [id] }
mysql:
  table: product
  modify: |
    self.with_condition(sql::ident("price") > 15)
"#;
    let err = db
        .vista_factory()
        .from_yaml(yaml)
        .map(drop)
        .expect_err("modify needs the rhai feature");
    let msg = err.to_string();
    assert!(
        msg.contains("modify") && msg.contains("`rhai` feature"),
        "{msg}"
    );
    Ok(())
}

/// Create suffixed `client` and `orders` tables and return their names.
async fn clients_orders(suffix: &str) -> (MysqlDB, String, String) {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let client_t = format!("vista_client_{suffix}");
    let orders_t = format!("vista_orders_{suffix}");

    for t in [&client_t, &orders_t] {
        sqlx::query(&format!("DROP TABLE IF EXISTS `{t}`"))
            .execute(db.pool())
            .await
            .unwrap();
    }
    sqlx::query(&format!(
        "CREATE TABLE `{client_t}` (id VARCHAR(255) PRIMARY KEY, name TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{orders_t}` (id VARCHAR(255) PRIMARY KEY, client_id VARCHAR(255) NOT NULL, total BIGINT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO `{client_t}` VALUES ('alice','Alice'),('bob','Bob')"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO `{orders_t}` VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    (db, client_t, orders_t)
}
