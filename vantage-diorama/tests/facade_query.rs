//! Facade narrowing: `dio.cached()` honours conditions (any operator), search
//! and order, pushing each into the master where it can answer it and
//! evaluating it over the rows read where it cannot.
//!
//! The CSV cases are the ones that matter most. CSV is the canonical
//! capability-poor master — it advertises no `Order`, and its factory flags
//! no column `Orderable` — so every one of these narrowings is served entirely
//! by the Dio. A mock master with the flags set by hand would pass while the
//! real thing failed.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Dio;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column, ColumnFlag, FilterOp, SortDirection, TableShell, Vista, VistaMetadata,
    forward_table_shell, mocks::MockShell,
};

mod support;

// ---- fixtures --------------------------------------------------------------

/// `tests/fixtures/products.csv` — Espresso 4, Cappuccino 5, Latte 6,
/// Flat White 5, in that file order.
fn csv_master() -> Result<Vista> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let csv = Csv::new(dir);
    let table = Table::<Csv, EmptyEntity>::new("products", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    csv.vista_factory().from_table(table)
}

fn row(name: &str, price: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("name".to_string(), CborValue::Text(name.to_string()));
    record.insert("price".to_string(), CborValue::Integer(price.into()));
    record
}

/// A master that *can* order and search server-side, for the push-down side.
fn capable_master() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
                .with_column(Column::new("price", "i64").with_flag(ColumnFlag::Orderable))
                .with_id_column("id"),
        )
        .with_capabilities(
            [Capability::Order, Capability::Search]
                .into_iter()
                .collect(),
        )
}

async fn eager_dio(master: Vista) -> Result<Dio> {
    Ok(support::eager_dio(master).await)
}

fn names(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    rows.values()
        .filter_map(|r| match r.get("name") {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

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

/// Conditions accrete and combine with an order, all locally.
#[tokio::test]
async fn csv_combines_condition_and_order() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let narrowed = dio
        .cached()
        .with_condition("price", FilterOp::Eq, 5)?
        .with_order("name", SortDirection::Descending)?;
    assert_eq!(
        names(&narrowed.list_values().await?),
        vec!["Flat White", "Cappuccino"],
    );
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

// ---- the capability contract -----------------------------------------------

/// A facade that advertises ordering must not then refuse it with
/// `Unimplemented`.
#[tokio::test]
async fn the_facade_honours_the_capabilities_it_advertises() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let facade = dio.cached();
    assert!(facade.capabilities().has(Capability::Order));

    let mut facade = facade;
    facade.set_order(Some(("name", SortDirection::Ascending)))?;
    assert_eq!(
        names(&facade.list_values().await?)
            .first()
            .map(String::as_str),
        Some("Cappuccino"),
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

    let at_master = dio.master().clone().with_order("name", SortDirection::Ascending)?;
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
