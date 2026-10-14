//! Positional windows over an ordered set, and the cursor SurrealDB doesn't
//! offer.

use vantage_vista::{Capability, SortDirection};

use super::{TestResult, name_of, product_table, seed_row, setup};

#[tokio::test]
async fn vista_fetch_window_returns_row_indexed_slice() -> TestResult {
    let (db, table_name) = setup().await;
    for (id, name) in [
        ("a", "Apple"),
        ("b", "Banana"),
        ("c", "Cherry"),
        ("d", "Date"),
    ] {
        seed_row(&db, &table_name, id, name, 1, false).await?;
    }

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    vista.set_order(Some(("name", SortDirection::Ascending)))?;

    let window = vista.fetch_window(1, 2).await?;
    let names: Vec<String> = window.rows.iter().map(|(_, r)| name_of(r)).collect();
    assert_eq!(names, vec!["Banana".to_string(), "Cherry".into()]);
    Ok(())
}

/// Consecutive windows page through an ordered set; the last is short.
#[tokio::test]
async fn vista_fetch_window_pages_through_the_set() -> TestResult {
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
    let vista = db
        .vista_factory()
        .from_table(table)?
        .with_order("name", SortDirection::Ascending)?;

    let names = |page: vantage_vista::Page| -> Vec<String> {
        page.rows.iter().map(|(_, r)| name_of(r)).collect()
    };
    assert_eq!(names(vista.fetch_window(0, 2).await?), ["Apple", "Banana"]);
    assert_eq!(names(vista.fetch_window(2, 2).await?), ["Cherry", "Date"]);
    assert_eq!(names(vista.fetch_window(4, 2).await?), ["Elder"]);
    Ok(())
}

/// SurrealDB windows by position; it has no cursor of its own.
#[tokio::test]
async fn vista_fetch_cursor_is_unsupported() -> TestResult {
    let (db, table_name) = setup().await;
    let vista = db
        .vista_factory()
        .from_table(product_table(db.clone(), &table_name))?;
    assert!(!vista.capabilities().has(Capability::Cursor));
    let err = vista
        .fetch_cursor(None, 2)
        .await
        .map(drop)
        .expect_err("refused");
    assert!(err.is_unsupported(), "{err:?}");
    Ok(())
}
