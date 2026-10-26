//! Order, quicksearch and windowed reads on the product vista.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_vista::{SortDirection, VistaFactory};

use super::{TestResult, product_table, setup};

#[tokio::test]
async fn vista_set_order_ascending_descending_and_clear() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // sort ascending by price → a (10), b (20), c (30)
    vista.set_order(Some(("price", SortDirection::Ascending)))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);

    // replace-semantics: switch to descending name → c (Gamma), b (Beta), a (Alpha)
    vista.set_order(Some(("name", SortDirection::Descending)))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["c", "b", "a"]);

    // clear → back to insertion order
    vista.set_order(None)?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

#[tokio::test]
async fn vista_set_order_rejects_unknown_column() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    let err = vista
        .set_order(Some(("not_a_column", SortDirection::Ascending)))
        .unwrap_err();
    assert!(!err.is_unsupported(), "an unknown column is a normal error");
    assert!(
        vista.query().order.is_none(),
        "a refused order isn't recorded"
    );
    Ok(())
}

#[tokio::test]
async fn vista_set_search_filters_results_with_replace_semantics() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // search "alpha" → only row a (name = "Alpha")
    vista.set_search(Some("alpha"))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a"], "search 'alpha' must match only row a");

    // replace-semantics: search "amma" → only row c (name = "Gamma")
    vista.set_search(Some("amma"))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(
        ids,
        ["c"],
        "search 'amma' must match only row c after replace"
    );

    // clear → all rows back
    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3, "clearing the search must restore all rows");
    Ok(())
}

/// Only `name` is flagged `searchable`, so a needle found only in `body`
/// matches nothing.
#[tokio::test]
async fn sqlite_search_reads_only_searchable_columns() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await?;
    sqlx::query("CREATE TABLE note (id TEXT PRIMARY KEY, name TEXT NOT NULL, body TEXT NOT NULL)")
        .execute(db.pool())
        .await?;
    sqlx::query("INSERT INTO note VALUES ('a', 'Alpha', 'hoverboard'), ('b', 'Beta', 'plutonium')")
        .execute(db.pool())
        .await?;
    let yaml = r#"
name: note
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [searchable] }
  body: { type: string }
"#;
    let mut vista = db.vista_factory().from_yaml(yaml)?;

    vista.set_search(Some("hover"))?;
    assert!(vista.list_values().await?.is_empty(), "body isn't searched");

    vista.set_search(Some("bet"))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["b"]);
    Ok(())
}

fn window_ids(page: &vantage_vista::Page) -> Vec<&str> {
    page.rows.iter().map(|(id, _)| id.as_str()).collect()
}

#[tokio::test]
async fn vista_fetch_window_offset_pagination() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_order(Some(("id", SortDirection::Ascending)))?;

    let first = vista.fetch_window(0, 2).await?;
    assert_eq!(window_ids(&first), ["a", "b"], "the first two rows");
    assert_eq!(first.total, None, "a window doesn't count the set");

    let second = vista.fetch_window(2, 2).await?;
    assert_eq!(window_ids(&second), ["c"], "the third row");

    let past_the_end = vista.fetch_window(4, 2).await?;
    assert!(past_the_end.rows.is_empty(), "nothing past the end");
    Ok(())
}

/// SQL pages by offset, so it has no cursor: `fetch_cursor` refuses.
#[tokio::test]
async fn vista_fetch_cursor_is_unsupported() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;

    let err = vista.fetch_cursor(None, 2).await.unwrap_err();
    assert!(err.is_unsupported());
    Ok(())
}

#[tokio::test]
async fn vista_fetch_window_honors_search_and_order() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_order(Some(("name", SortDirection::Descending)))?;
    vista.set_search(Some("a"))?; // matches Alpha, Beta, Gamma (all contain 'a' case-insensitively)

    let page = vista.fetch_window(0, 10).await?;
    let names: Vec<String> = page
        .rows
        .iter()
        .map(|(_, rec)| match rec.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Gamma".to_string(), "Beta".to_string(), "Alpha".to_string()],
        "page must honour both search and DESC order on name"
    );
    Ok(())
}
