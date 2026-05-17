//! Vista integration: typed `Table<SurrealDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running SurrealDB. Set `SURREALDB_URL` (defaults to
//! `cbor://root:root@localhost:8000/bakery/v2`). Each test runs in a fresh
//! randomised database so parallel runs don't trigger transaction conflicts.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use uuid::Uuid;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

use surreal_client::SurrealConnection;

type TestResult = std::result::Result<(), Box<dyn Error>>;

/// Build a per-test DSN with an isolated database name. The base DSN comes
/// from `SURREALDB_URL` (root auth + host); each test substitutes its own
/// database to avoid transaction conflicts between parallel runs.
fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn setup() -> (SurrealDB, String) {
    let database = format!("vista_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let table = format!("product_{}", Uuid::new_v4().simple());
    (db, table)
}

fn product_table(db: SurrealDB, table_name: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::<SurrealDB, EmptyEntity>::new(table_name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

async fn seed_row(
    db: &SurrealDB,
    table_name: &str,
    id: &str,
    name: &str,
    price: i64,
    is_deleted: bool,
) -> std::result::Result<Thing, Box<dyn Error>> {
    let record = format!("{}:{}", table_name, id);
    let name_owned = name.to_string();
    db.execute(&surreal_expr!(
        &format!(
            "CREATE {} SET name = {{}}, price = {{}}, is_deleted = {{}}",
            record
        ),
        name_owned,
        price,
        is_deleted
    ))
    .await?;
    Ok(Thing::new(table_name, id))
}

#[tokio::test]
async fn vista_lists_typed_surreal_as_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, true).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), table_name);
    assert_eq!(vista.get_id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    let alpha_key = format!("{}:alpha", table_name);
    let alpha = rows.get(&alpha_key).expect("row alpha");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    assert_eq!(alpha.get("price"), Some(&CborValue::Integer(10i64.into())));
    assert_eq!(alpha.get("is_deleted"), Some(&CborValue::Bool(false)));

    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    // Bare id — shell prefixes with table name.
    let row = vista.get_value(&"alpha".to_string()).await?.expect("found");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Alpha".to_string())));

    // Full record id also works.
    let full = format!("{}:alpha", table_name);
    let row2 = vista.get_value(&full).await?.expect("found");
    assert_eq!(
        row2.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );

    let missing = vista.get_value(&"nope".to_string()).await?;
    assert!(missing.is_none());

    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "A", 10, false).await?;
    seed_row(&db, &table_name, "b", "B", 20, true).await?;
    seed_row(&db, &table_name, "c", "C", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "biff", "Biff", 99, false).await?;

    let yaml = format!(
        r#"
name: product_view
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
surreal:
  table: {}
"#,
        table_name
    );

    let vista = db.vista_factory().from_yaml(&yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Biff".to_string())));

    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Delta".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value(&"delta".to_string(), &record).await?;

    let fetched = vista
        .get_value(&"delta".to_string())
        .await?
        .expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Delta".into())));
    assert_eq!(
        fetched.get("price"),
        Some(&CborValue::Integer(99i64.into()))
    );

    vista.delete(&"delta".to_string()).await?;
    assert!(vista.get_value(&"delta".to_string()).await?.is_none());

    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    assert!(!caps.can_subscribe);
    assert_eq!(vista.driver(), "surrealdb");

    Ok(())
}

#[tokio::test]
async fn vista_eq_condition_with_typed_value() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "Match", 10, false).await?;
    seed_row(&db, &table_name, "b", "Other", 20, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    vista.add_condition_eq("name", CborValue::Text("Match".into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Match".into())));

    Ok(())
}

#[tokio::test]
async fn vista_unknown_field_eq_errors() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    let err = vista
        .add_condition_eq("nonexistent", CborValue::Bool(true))
        .expect_err("should reject unknown field");
    assert!(err.to_string().contains("nonexistent"));

    Ok(())
}

#[tokio::test]
async fn vista_add_order_filters_results_with_replace_semantics() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_order);

    vista.add_order("price", SortDirection::Ascending)?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows
        .values()
        .map(|r| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Alpha".to_string(), "Beta".into(), "Gamma".into()]
    );

    vista.add_order("name", SortDirection::Descending)?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows
        .values()
        .map(|r| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Gamma".to_string(), "Beta".into(), "Alpha".into()]
    );

    vista.clear_orders()?;
    let _rows = vista.list_values().await?;
    Ok(())
}

#[tokio::test]
async fn vista_add_search_uses_string_contains() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_search);

    vista.add_search("amma")?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows.len(),
        1,
        "only Gamma contains 'amma' (case-insensitive)"
    );

    vista.add_search("alpha")?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);

    vista.clear_search()?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

#[tokio::test]
async fn vista_fetch_page_offset_pagination() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    for (id, name) in [
        ("a", "Apple"),
        ("b", "Banana"),
        ("c", "Cherry"),
        ("d", "Date"),
        ("e", "Elder"),
    ] {
        seed_row(&db, &table_name, id, name, 1, false).await?;
    }

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_fetch_page);

    vista.set_page_size(2)?;
    vista.add_order("name", SortDirection::Ascending)?;

    let p1 = vista.fetch_page(1).await?;
    let names_p1: Vec<String> = p1
        .iter()
        .map(|(_, r)| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names_p1, vec!["Apple".to_string(), "Banana".into()]);

    let p2 = vista.fetch_page(2).await?;
    let names_p2: Vec<String> = p2
        .iter()
        .map(|(_, r)| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names_p2, vec!["Cherry".to_string(), "Date".into()]);

    let p3 = vista.fetch_page(3).await?;
    assert_eq!(p3.len(), 1);
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_chains_pages_until_exhausted() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    for (id, name) in [("a", "Apple"), ("b", "Banana"), ("c", "Cherry")] {
        seed_row(&db, &table_name, id, name, 1, false).await?;
    }

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_fetch_next);

    vista.set_page_size(2)?;
    vista.add_order("name", SortDirection::Ascending)?;

    let (r1, tok1) = vista.fetch_next(None).await?;
    assert_eq!(r1.len(), 2);
    assert!(tok1.is_some());

    let (r2, tok2) = vista.fetch_next(tok1).await?;
    assert_eq!(r2.len(), 1);
    assert!(tok2.is_none());
    Ok(())
}
