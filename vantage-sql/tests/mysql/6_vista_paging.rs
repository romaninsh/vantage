//! Vista ordering, quicksearch and paging over MySQL.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.
//! Each test uses a uniquely-suffixed table and drops it when done.

#![cfg(feature = "vista")]

use std::error::Error;

use vantage_dataset::prelude::*;
use vantage_sql::mysql::MysqlDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{SortDirection, Vista};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// Five rows; `price` is deliberately non-unique (10, 20, 20, 20, 30).
async fn setup(suffix: &str) -> (MysqlDB, String) {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let table_name = format!("vista_paging_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table_name))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{}` (
            id VARCHAR(255) PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL
        )",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES \
         ('a', 'Alpha', 10), \
         ('b', 'Beta', 20), \
         ('c', 'Gamma', 20), \
         ('d', 'Delta', 20), \
         ('e', 'Epsilon', 30)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    (db, table_name)
}

async fn teardown(db: &MysqlDB, name: &str) {
    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", name))
        .execute(db.pool())
        .await
        .unwrap();
}

fn product_vista(db: &MysqlDB, name: &str) -> Vista {
    let table = Table::<MysqlDB, EmptyEntity>::new(name, db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    db.vista_factory().from_table(table).unwrap()
}

async fn ids(vista: &Vista) -> Vec<String> {
    vista.list_values().await.unwrap().into_keys().collect()
}

fn page_ids(rows: &[(String, vantage_types::Record<ciborium::Value>)]) -> Vec<String> {
    rows.iter().map(|(id, _)| id.clone()).collect()
}

#[tokio::test]
async fn paging_order_ascending_descending_and_clear() -> TestResult {
    let (db, name) = setup("order").await;
    let mut vista = product_vista(&db, &name);

    vista.add_order("name", SortDirection::Ascending)?;
    assert_eq!(ids(&vista).await, ["a", "b", "d", "e", "c"]);

    // Replace semantics: the new order supersedes the previous one.
    vista.add_order("name", SortDirection::Descending)?;
    assert_eq!(ids(&vista).await, ["c", "e", "d", "b", "a"]);

    vista.clear_orders()?;
    assert_eq!(ids(&vista).await.len(), 5);

    assert!(vista.add_order("nope", SortDirection::Ascending).is_err());
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_search_replaces_and_clears() -> TestResult {
    let (db, name) = setup("search").await;
    let mut vista = product_vista(&db, &name);

    vista.add_search("alpha")?;
    assert_eq!(ids(&vista).await, ["a"]);

    vista.add_search("amma")?;
    assert_eq!(ids(&vista).await, ["c"]);

    vista.add_search("zzz-no-match")?;
    assert!(ids(&vista).await.is_empty());

    vista.clear_search()?;
    assert_eq!(ids(&vista).await.len(), 5);
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_search_on_table_without_columns_matches_nothing() -> TestResult {
    let (db, name) = setup("nosearch").await;
    let table = Table::<MysqlDB, EmptyEntity>::new(&name, db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.add_search("alpha")?;
    assert!(ids(&vista).await.is_empty());
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_fetch_page_honours_page_size_search_and_order() -> TestResult {
    let (db, name) = setup("page").await;
    let mut vista = product_vista(&db, &name);

    assert!(vista.fetch_page(1).await.is_err(), "page size not set");
    assert!(vista.set_page_size(0).is_err());

    vista.set_page_size(2)?;
    assert!(vista.fetch_page(0).await.is_err(), "pages are 1-based");
    vista.add_order("name", SortDirection::Ascending)?;

    assert_eq!(page_ids(&vista.fetch_page(1).await?), ["a", "b"]);
    assert_eq!(page_ids(&vista.fetch_page(2).await?), ["d", "e"]);
    assert_eq!(page_ids(&vista.fetch_page(3).await?), ["c"]);
    assert!(vista.fetch_page(4).await?.is_empty());

    // 'a' occurs in Alpha, Beta, Gamma, Delta; Epsilon has none.
    vista.add_search("a")?;
    vista.add_order("name", SortDirection::Descending)?;
    assert_eq!(page_ids(&vista.fetch_page(1).await?), ["c", "d"]);
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_fetch_window() -> TestResult {
    let (db, name) = setup("window").await;
    let mut vista = product_vista(&db, &name);
    vista.add_order("name", SortDirection::Ascending)?;

    assert_eq!(page_ids(&vista.fetch_window(1, 3).await?), ["b", "d", "e"]);
    assert_eq!(page_ids(&vista.fetch_window(4, 10).await?), ["c"]);
    assert!(vista.fetch_window(9, 2).await?.is_empty());
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_fetch_next_over_non_unique_sort_has_no_gaps_or_repeats() -> TestResult {
    let (db, name) = setup("next").await;
    let mut vista = product_vista(&db, &name);

    assert!(vista.fetch_next(None).await.is_err(), "page size not set");
    vista.set_page_size(2)?;
    // `price` ties three rows; the id tiebreak keeps the chain stable.
    vista.add_order("price", SortDirection::Ascending)?;

    let mut seen = Vec::new();
    let mut token = None;
    loop {
        let (rows, next) = vista.fetch_next(token).await?;
        seen.extend(page_ids(&rows));
        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    assert_eq!(seen, ["a", "b", "c", "d", "e"]);

    // Unordered paging is also deterministic (falls back to the id column).
    vista.clear_orders()?;
    let first = page_ids(&vista.fetch_page(1).await?);
    let second = page_ids(&vista.fetch_page(2).await?);
    let third = page_ids(&vista.fetch_page(3).await?);
    assert_eq!([first, second, third].concat(), ["a", "b", "c", "d", "e"]);

    let bad = Some(ciborium::Value::Text("x".into()));
    assert!(vista.fetch_next(bad).await.is_err());
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_capabilities_advertised() -> TestResult {
    let (db, name) = setup("caps").await;
    let vista = product_vista(&db, &name);
    let caps = vista.capabilities();
    assert!(caps.can_order);
    assert!(caps.can_search);
    assert!(caps.can_set_page_size);
    assert!(caps.can_fetch_page);
    assert!(caps.can_fetch_window);
    assert!(caps.can_fetch_next);
    teardown(&db, &name).await;
    Ok(())
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn paging_query_sourced_vista_keeps_read_capabilities() -> TestResult {
    use vantage_vista::VistaFactory;

    let (db, name) = setup("rhai").await;
    let yaml = format!(
        r#"
name: paging_rhai
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
mysql:
  rhai: |
    select().from("{name}").field("id").field("name")
"#
    );
    let mut vista = db.vista_factory().from_yaml(&yaml)?;

    let caps = vista.capabilities();
    assert!(!caps.can_insert);
    assert!(caps.can_order && caps.can_search);
    assert!(caps.can_set_page_size && caps.can_fetch_page);
    assert!(caps.can_fetch_window && caps.can_fetch_next);

    vista.set_page_size(2)?;
    vista.add_order("name", SortDirection::Ascending)?;
    assert_eq!(page_ids(&vista.fetch_page(1).await?), ["a", "b"]);
    vista.add_search("eps")?;
    assert_eq!(ids(&vista).await, ["e"]);
    teardown(&db, &name).await;
    Ok(())
}
