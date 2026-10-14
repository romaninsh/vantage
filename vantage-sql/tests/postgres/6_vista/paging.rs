//! Order and windowed reads: `set_order`, and window scans that stay stable
//! with no caller order and with a non-unique id column.

use vantage_dataset::prelude::*;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, SortDirection};

use super::{TestResult, product_table, setup};

#[tokio::test]
async fn vista_set_order_ascending_descending_and_clear() -> TestResult {
    let (db, name) = setup("order").await;
    let table = product_table(db.clone(), &name);
    let mut vista = db.vista_factory().from_table(table)?;

    // A table-sourced Postgres Vista advertises ordering and flags every column
    // ORDERABLE, so `set_order` works without going through YAML.
    assert!(vista.capabilities().has(Capability::Order));

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

    // clear → every row still present (order unconstrained)
    vista.set_order(None)?;
    let rows = vista.list_values().await?;
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

/// A window scan must visit every row even when the id column is not unique.
///
/// A window keys rows by id, so duplicates collapse and a FULL SQL window
/// can arrive under-length. Only an empty window ends the scan.
#[tokio::test]
async fn vista_window_scan_survives_a_non_unique_id_column() -> TestResult {
    let (db, name) = setup("dup_ids").await;
    // Page by `name`, which repeats: 'dup' twice, then a distinct third row.
    sqlx::query(&format!("DELETE FROM \"{}\"", name))
        .execute(db.pool())
        .await?;
    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES \
         ('a', 'dup', 10, false), \
         ('b', 'dup', 20, false), \
         ('c', 'tail', 30, false)",
        name
    ))
    .execute(db.pool())
    .await?;

    let table = Table::<PostgresDB, EmptyEntity>::new(&name, db.clone())
        .with_id_column("name")
        .with_column_of::<String>("id")
        .with_column_of::<i64>("price");
    let mut vista = db.vista_factory().from_table(table)?;
    vista.set_order(Some(("id", SortDirection::Ascending)))?;

    let mut seen: Vec<String> = Vec::new();
    for offset in (0..20).step_by(2) {
        let page = vista.fetch_window(offset, 2).await?;
        if page.rows.is_empty() {
            break;
        }
        seen.extend(page.rows.into_iter().map(|(id, _)| id));
    }
    // 'dup' collapses (the first window returns a single record), but the
    // scan must still reach 'tail'.
    assert!(
        seen.contains(&"tail".to_string()),
        "scan ended early on a deduped window; saw {seen:?}"
    );
    Ok(())
}

/// Paging with no caller-supplied order must still be a stable window:
/// `LIMIT`/`OFFSET` without `ORDER BY` lets Postgres return rows in any
/// order, so pages could repeat one row and skip another.
#[tokio::test]
async fn vista_paging_without_an_order_still_visits_every_row_once() -> TestResult {
    let (db, name) = setup("unordered_pages").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let mut seen: Vec<String> = Vec::new();
    for offset in [0, 2] {
        let page = vista.fetch_window(offset, 2).await?;
        seen.extend(page.rows.into_iter().map(|(id, _)| id));
    }
    seen.sort();
    assert_eq!(seen, ["a", "b", "c"], "each id must appear exactly once");
    Ok(())
}
