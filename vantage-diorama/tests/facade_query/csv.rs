//! A CSV master can order, filter and search none of this: the facade
//! answers every clause over the cache, and still advertises it.

use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::{Capability, FilterOp, SortDirection};

use crate::{csv_master, eager_dio, names, row};

// ---- CSV: a master that can do none of this --------------------------------

/// Sorting a CSV-backed Dio. The CSV driver cannot order, and flags no column
/// orderable, so the facade orders the cache itself.
#[tokio::test]
async fn csv_sorts_over_the_cache() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    assert!(
        !dio.master().capabilities().has(Capability::Order),
        "the CSV master genuinely cannot order — that is the point of this test",
    );

    let mut by_name = dio.cached();
    by_name.set_order(Some(("name", SortDirection::Ascending)))?;
    assert_eq!(
        names(&by_name.list_values().await?),
        vec!["Cappuccino", "Espresso", "Flat White", "Latte"],
    );

    let mut by_price = dio.cached();
    by_price.set_order(Some(("price", SortDirection::Descending)))?;
    assert_eq!(
        names(&by_price.list_values().await?),
        // 6, then the two 5s (tie broken on id: p2 before p4), then 4.
        vec!["Latte", "Cappuccino", "Flat White", "Espresso"],
    );
    Ok(())
}

/// Prices are integers; ordering them must be numeric. A comparison that fell
/// through to a debug-string compare would rank `10` below `4`.
#[tokio::test]
async fn csv_sorts_numbers_numerically() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    // A double-digit price alongside the single-digit fixture rows.
    dio.apply_change(vantage_diorama::ChangeEvent::Inserted {
        id: "p9".into(),
        new: Some(row("Magnum", 10)),
    })
    .await?;

    let mut by_price = dio.cached();
    by_price.set_order(Some(("price", SortDirection::Ascending)))?;
    assert_eq!(
        names(&by_price.list_values().await?)
            .last()
            .map(String::as_str),
        Some("Magnum"),
        "10 must sort above 6, not below 4",
    );
    Ok(())
}

#[tokio::test]
async fn csv_filters_over_the_cache() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let fivers = dio.cached().with_condition("price", FilterOp::Eq, 5)?;
    let mut got = names(&fivers.list_values().await?);
    got.sort();
    assert_eq!(got, vec!["Cappuccino", "Flat White"]);
    assert_eq!(fivers.count().await?, 2, "count reflects the narrowing");
    Ok(())
}

/// A narrowed handle is a smaller set: an id outside it is simply not there.
#[tokio::test]
async fn csv_narrowing_hides_rows_from_get() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let cheap = dio.cached().with_condition("price", FilterOp::Eq, 4)?;

    assert!(cheap.get_value("p1").await?.is_some(), "Espresso is 4");
    assert!(
        cheap.get_value("p3").await?.is_none(),
        "Latte is 6 — outside the set"
    );
    Ok(())
}

/// CSV has no search; the facade advertises one anyway and evaluates it over
/// the cache, case-insensitively, across every cell.
#[tokio::test]
async fn csv_searches_over_the_cache() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let facade = dio.cached();
    assert!(facade.capabilities().has(Capability::Search));

    let searched = facade.with_search("ESP")?;
    assert_eq!(names(&searched.list_values().await?), vec!["Espresso"]);
    assert_eq!(searched.count().await?, 1);
    Ok(())
}

/// Operators other than equality are evaluated over the cache too, and
/// combine with the order.
#[tokio::test]
async fn csv_takes_operator_conditions() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    assert!(dio.cached().capabilities().has(Capability::FilterOps));

    let dear = dio
        .cached()
        .with_condition("price", FilterOp::Gt, 4)?
        .with_condition("price", FilterOp::Lt, 6)?
        .with_order("name", SortDirection::Ascending)?;
    assert_eq!(
        names(&dear.list_values().await?),
        vec!["Cappuccino", "Flat White"]
    );
    let page = dear.fetch_window(1, 5).await?;
    assert_eq!(page.total, Some(2));
    assert_eq!(page.rows.len(), 1);
    Ok(())
}
