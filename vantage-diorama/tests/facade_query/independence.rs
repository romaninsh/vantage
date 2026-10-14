//! Narrowing is per handle: facades over one Dio, and clones of one facade,
//! never disturb each other.

use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::{FilterOp, SortDirection, Vista};

use crate::{capable_master, csv_master, eager_dio, names, row};

// ---- independence -----------------------------------------------------------

/// Narrowing is per-handle. Two facades over one Dio must not disturb each
/// other — the shared-mutation trap an in-place setter falls into.
#[tokio::test]
async fn two_facades_narrow_independently() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let mut ascending = dio.cached();
    ascending.set_order(Some(("name", SortDirection::Ascending)))?;
    let mut descending = dio.cached();
    descending.set_order(Some(("name", SortDirection::Descending)))?;

    assert_eq!(
        names(&ascending.list_values().await?)
            .first()
            .map(String::as_str),
        Some("Cappuccino")
    );
    assert_eq!(
        names(&descending.list_values().await?)
            .first()
            .map(String::as_str),
        Some("Latte")
    );
    assert_eq!(
        dio.cached().list_values().await?.len(),
        4,
        "a fresh facade is unnarrowed"
    );
    Ok(())
}

/// A clone carries the narrowing but stays independent.
#[tokio::test]
async fn a_cloned_facade_inherits_but_does_not_share() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let base = dio.cached().with_condition("price", FilterOp::Eq, 5)?;
    let cloned = base.clone().with_order("name", SortDirection::Descending)?;

    assert_eq!(
        names(&cloned.list_values().await?),
        vec!["Flat White", "Cappuccino"],
        "the clone inherited the filter and added its own order",
    );
    assert_eq!(
        base.list_values().await?.len(),
        2,
        "the base kept its own (unordered) view"
    );
    Ok(())
}

/// An unnarrowed facade is untouched: still a plain cache read.
#[tokio::test]
async fn an_unnarrowed_facade_reads_the_cache() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    let dio = eager_dio(Vista::new("items", shell.clone())).await?;

    shell.set_record("z", row("zeta", 9));
    assert_eq!(
        dio.cached().list_values().await?.len(),
        1,
        "the cache-first contract is unchanged for an unnarrowed handle",
    );
    Ok(())
}
