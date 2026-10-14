//! A master that can answer a clause answers it; an order alone stays on the
//! cache; the preview names which side of the split each clause landed on.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection, TableShell, Vista, forward_table_shell};

use crate::{capable_master, csv_master, eager_dio, names, row};

/// A search the master can run is run there, over the master's whole set.
#[tokio::test]
async fn a_capable_master_answers_the_search_itself() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    let dio = eager_dio(Vista::new("items", shell.clone())).await?;
    shell.set_record("c", row("beta", 2));

    let searched = dio.cached().with_search("bet")?;
    assert_eq!(names(&searched.list_values().await?), vec!["beta"]);
    Ok(())
}

// ---- push-down side ---------------------------------------------------------

/// An order on its own never moves a read off the cache, even when the master
/// could sort: re-sorting never changes which rows a handle shows, and every
/// read method agrees on them. Observable because the master holds a row the
/// cache never received.
#[tokio::test]
async fn an_order_alone_reads_the_cache() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    shell.set_record("b", row("alpha", 1));
    let dio = eager_dio(Vista::new("items", shell.clone())).await?;

    // Appears upstream only — the cache does not have it.
    shell.set_record("c", row("beta", 2));

    let ordered = dio.cached().with_order("name", SortDirection::Ascending)?;
    let listed = ordered.list_values().await?;
    assert_eq!(names(&listed), vec!["alpha", "gamma"]);
    assert_eq!(ordered.count().await?, listed.len() as i64);
    assert_eq!(
        ordered.fetch_window(0, 10).await?.total,
        Some(listed.len() as i64)
    );
    assert!(ordered.get_value("c").await?.is_none(), "not cached");
    Ok(())
}

/// A master that lists its rows by name, case-insensitively, as a SQL
/// collation does.
#[derive(Clone)]
struct CaseInsensitive {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(CaseInsensitive, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let mut rows = self.inner.list().await?;
        let key = |row: &Record<CborValue>| match row.get("name") {
            Some(CborValue::Text(t)) => t.to_lowercase(),
            _ => String::new(),
        };
        rows.sort_by(|_, a, _, b| key(a).cmp(&key(b)));
        Ok(rows)
    }
});

/// A cache read sorts with vista's evaluator, in byte order, whatever the
/// master's collation; a read the master answers keeps the master's order.
/// The facade doesn't reconcile the two.
#[tokio::test]
async fn a_cache_read_orders_by_the_evaluator_not_the_master() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("apple", 1));
    shell.set_record("b", row("Banana", 1));
    let master = Vista::new("fruit", shell).map_shell(|inner| CaseInsensitive { inner });
    let dio = eager_dio(master).await?;

    let at_master = dio
        .master()
        .clone()
        .with_order("name", SortDirection::Ascending)?;
    assert_eq!(names(&at_master.list_values().await?), ["apple", "Banana"]);

    let cached = dio.cached().with_order("name", SortDirection::Ascending)?;
    assert_eq!(names(&cached.list_values().await?), ["Banana", "apple"]);

    // A condition the master takes sends the read there, order and all.
    let pushed = dio
        .cached()
        .with_condition("price", FilterOp::Eq, 1)?
        .with_order("name", SortDirection::Ascending)?;
    assert_eq!(names(&pushed.list_values().await?), ["apple", "Banana"]);
    Ok(())
}

/// A condition the master takes is answered at the source, over the
/// master's whole set.
#[tokio::test]
async fn a_capable_master_answers_a_condition_itself() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    let dio = eager_dio(Vista::new("items", shell.clone())).await?;
    shell.set_record("c", row("beta", 2));

    let beta = dio.cached().with_condition("name", FilterOp::Eq, "beta")?;
    assert_eq!(names(&beta.list_values().await?), vec!["beta"]);
    assert_eq!(beta.count().await?, 1);
    assert!(beta.get_value("c").await?.is_some());
    Ok(())
}

// ---- preview: which side of the split a clause landed on -------------------

/// A clause the master accepted belongs in the previewed master query, not in
/// the facade's local list. Attributing a pushed-down filter to the cache would
/// tell a reader the server never saw it.
#[tokio::test]
async fn preview_reports_a_pushed_down_condition_on_the_master() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    let dio = eager_dio(Vista::new("items", shell.clone())).await?;

    let narrowed = dio
        .cached()
        .with_condition("name", FilterOp::Eq, "gamma")?
        .with_order("price", SortDirection::Descending)?;

    let preview = narrowed.preview();
    assert!(
        preview
            .query
            .as_deref()
            .is_some_and(|q| q.contains("gamma")),
        "the master accepted the filter, so it must appear there: {preview:?}",
    );
    assert!(
        !preview
            .note
            .as_deref()
            .is_some_and(|n| n.contains("applied over the cache")),
        "nothing was refused, so nothing is applied over the cache: {preview:?}",
    );
    Ok(())
}

/// The mirror case. CSV can neither order nor filter here, so every clause is
/// answered over the cache and the master query stays bare.
#[tokio::test]
async fn preview_reports_a_refused_order_on_the_facade() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let narrowed = dio.cached().with_order("name", SortDirection::Ascending)?;

    let preview = narrowed.preview();
    assert!(
        preview
            .note
            .as_deref()
            .is_some_and(|n| n.contains("applied over the cache: order by name asc")),
        "CSV refused the order, so it is applied locally: {preview:?}",
    );
    assert_eq!(narrowed.driver_name(), "dio");
    Ok(())
}
