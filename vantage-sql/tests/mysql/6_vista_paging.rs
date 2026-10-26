//! Vista ordering, quicksearch and windowed reads over MySQL.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.
//! Each test uses a uniquely-suffixed table and drops it when done.

#![cfg(feature = "vista")]

use std::error::Error;

use vantage_dataset::prelude::*;
use vantage_sql::mysql::MysqlDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, Page, SortDirection, Vista};

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

fn page_ids(page: &Page) -> Vec<String> {
    page.rows.iter().map(|(id, _)| id.clone()).collect()
}

#[tokio::test]
async fn paging_order_ascending_descending_and_clear() -> TestResult {
    let (db, name) = setup("order").await;
    let mut vista = product_vista(&db, &name);

    vista.set_order(Some(("name", SortDirection::Ascending)))?;
    assert_eq!(ids(&vista).await, ["a", "b", "d", "e", "c"]);

    // Replace semantics: the new order supersedes the previous one.
    vista.set_order(Some(("name", SortDirection::Descending)))?;
    assert_eq!(ids(&vista).await, ["c", "e", "d", "b", "a"]);

    vista.set_order(None)?;
    assert_eq!(ids(&vista).await.len(), 5);

    assert!(
        vista
            .set_order(Some(("nope", SortDirection::Ascending)))
            .is_err()
    );
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_search_replaces_and_clears() -> TestResult {
    let (db, name) = setup("search").await;
    let mut vista = product_vista(&db, &name);

    vista.set_search(Some("alpha"))?;
    assert_eq!(ids(&vista).await, ["a"]);

    vista.set_search(Some("amma"))?;
    assert_eq!(ids(&vista).await, ["c"]);

    vista.set_search(Some("zzz-no-match"))?;
    assert!(ids(&vista).await.is_empty());

    vista.set_search(None)?;
    assert_eq!(ids(&vista).await.len(), 5);
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_search_on_table_without_columns_matches_nothing() -> TestResult {
    let (db, name) = setup("nosearch").await;
    let table = Table::<MysqlDB, EmptyEntity>::new(&name, db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_search(Some("alpha"))?;
    assert!(ids(&vista).await.is_empty());
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_fetch_window_honours_search_and_order() -> TestResult {
    let (db, name) = setup("window").await;
    let mut vista = product_vista(&db, &name);
    vista.set_order(Some(("name", SortDirection::Ascending)))?;

    assert_eq!(page_ids(&vista.fetch_window(0, 2).await?), ["a", "b"]);
    assert_eq!(page_ids(&vista.fetch_window(1, 3).await?), ["b", "d", "e"]);
    assert_eq!(page_ids(&vista.fetch_window(4, 10).await?), ["c"]);
    assert!(vista.fetch_window(9, 2).await?.rows.is_empty());

    // 'a' occurs in Alpha, Beta, Gamma, Delta; Epsilon has none.
    vista.set_search(Some("a"))?;
    vista.set_order(Some(("name", SortDirection::Descending)))?;
    assert_eq!(page_ids(&vista.fetch_window(0, 2).await?), ["c", "d"]);
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn paging_windows_over_non_unique_sort_have_no_gaps_or_repeats() -> TestResult {
    let (db, name) = setup("ties").await;
    let mut vista = product_vista(&db, &name);
    // `price` ties three rows; the id tiebreak keeps the windows stable.
    vista.set_order(Some(("price", SortDirection::Ascending)))?;

    let mut seen = Vec::new();
    for offset in [0, 2, 4] {
        seen.extend(page_ids(&vista.fetch_window(offset, 2).await?));
    }
    assert_eq!(seen, ["a", "b", "c", "d", "e"]);

    // Unordered windows are also deterministic (they fall back to the id).
    vista.set_order(None)?;
    let mut seen = Vec::new();
    for offset in [0, 2, 4] {
        seen.extend(page_ids(&vista.fetch_window(offset, 2).await?));
    }
    assert_eq!(seen, ["a", "b", "c", "d", "e"]);
    teardown(&db, &name).await;
    Ok(())
}

/// Offset paging only: there is no cursor.
#[tokio::test]
async fn fetch_cursor_is_unsupported() -> TestResult {
    let (db, name) = setup("caps").await;
    let vista = product_vista(&db, &name);
    assert!(
        vista
            .fetch_cursor(None, 2)
            .await
            .unwrap_err()
            .is_unsupported()
    );
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
    sql::select().from("{name}").field("id").field("name")
"#
    );
    let mut vista = db.vista_factory().from_yaml(&yaml)?;

    let caps = vista.capabilities();
    assert!(!caps.has(Capability::Insert));
    assert!(caps.has(Capability::Order) && caps.has(Capability::Search));
    assert!(caps.has(Capability::Window));

    vista.set_order(Some(("name", SortDirection::Ascending)))?;
    assert_eq!(page_ids(&vista.fetch_window(0, 2).await?), ["a", "b"]);
    vista.set_search(Some("eps"))?;
    assert_eq!(ids(&vista).await, ["e"]);
    teardown(&db, &name).await;
    Ok(())
}
