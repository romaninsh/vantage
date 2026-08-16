//! Facade narrowing: `dio.vista()` honours condition and order, pushing each
//! into the master where it can answer it and applying it over the cache where
//! it cannot. Search is not lifted — see
//! `the_facade_refuses_a_search_it_cannot_answer`.
//!
//! The CSV cases are the ones that matter most. CSV is the canonical
//! capability-poor master — it advertises no `can_order`, and its factory flags
//! no column `ORDERABLE` — so every one of these narrowings is served entirely
//! by the Dio. A mock master with the flags set by hand would pass while the
//! real thing failed.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column, SortDirection, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell,
};

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
                .with_column(
                    Column::new("name", "String").with_flag(vantage_vista::flags::ORDERABLE),
                )
                .with_column(Column::new("price", "i64").with_flag(vantage_vista::flags::ORDERABLE))
                .with_id_column("id"),
        )
        .with_capabilities(VistaCapabilities {
            can_order: true,
            can_search: true,
            ..Default::default()
        })
}

async fn eager_dio(master: Vista) -> Result<Dio> {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    );
    lens.make_dio(master).await
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
/// orderable — before this, `add_order` was refused outright.
#[tokio::test]
async fn csv_sorts_over_the_cache() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    assert!(
        !dio.master().capabilities().can_order,
        "the CSV master genuinely cannot order — that is the point of this test",
    );

    let mut by_name = dio.vista();
    by_name.add_order("name", SortDirection::Ascending)?;
    assert_eq!(
        names(&by_name.list_values().await?),
        vec!["Cappuccino", "Espresso", "Flat White", "Latte"],
    );

    let mut by_price = dio.vista();
    by_price.add_order("price", SortDirection::Descending)?;
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
    dio.patched("p9", row("Magnum", 10)).await?;

    let mut by_price = dio.vista();
    by_price.add_order("price", SortDirection::Ascending)?;
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

    let mut fivers = dio.vista();
    fivers.add_condition_eq("price", CborValue::Integer(5.into()))?;
    let mut got = names(&fivers.list_values().await?);
    got.sort();
    assert_eq!(got, vec!["Cappuccino", "Flat White"]);
    assert_eq!(fivers.get_count().await?, 2, "count reflects the narrowing");
    Ok(())
}

/// Conditions accrete and combine with an order, all locally.
#[tokio::test]
async fn csv_combines_condition_and_order() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let mut narrowed = dio.vista();
    narrowed.add_condition_eq("price", CborValue::Integer(5.into()))?;
    narrowed.add_order("name", SortDirection::Descending)?;
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
    let mut cheap = dio.vista();
    cheap.add_condition_eq("price", CborValue::Integer(4.into()))?;

    assert!(cheap.get_value("p1").await?.is_some(), "Espresso is 4");
    assert!(
        cheap.get_value("p3").await?.is_none(),
        "Latte is 6 — outside the set"
    );
    Ok(())
}

// ---- the capability contract -----------------------------------------------

/// The regression this change exists for: the facade advertised `can_order`
/// and then refused it with `Unimplemented`.
#[tokio::test]
async fn the_facade_honours_the_capabilities_it_advertises() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let facade = dio.vista();
    assert!(facade.capabilities().can_order);

    let mut facade = facade;
    facade.add_order("name", SortDirection::Ascending)?;
    assert_eq!(
        names(&facade.list_values().await?)
            .first()
            .map(String::as_str),
        Some("Cappuccino"),
    );
    Ok(())
}

/// The facade does NOT lift search the way it lifts ordering. It has no
/// search of its own and nowhere to forward the term to, so it advertises
/// `can_search: false` and refuses the call — rather than accepting it and
/// handing back the unfiltered set, which is what a silent local filter
/// amounted to once the term could not be pushed down.
#[tokio::test]
async fn the_facade_refuses_a_search_it_cannot_answer() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;
    let facade = dio.vista();
    assert!(!facade.capabilities().can_search);

    let mut facade = facade;
    assert!(
        facade.add_search("esp").is_err(),
        "an accepted search that never runs is worse than a refused one",
    );
    Ok(())
}

// ---- push-down side ---------------------------------------------------------

/// The same calls against a master that *can* order and search are answered at
/// the source instead. Observable because the master holds a row the cache
/// never received.
#[tokio::test]
async fn a_capable_master_answers_the_narrowing_itself() -> Result<()> {
    let shell = capable_master();
    shell.set_record("a", row("gamma", 3));
    shell.set_record("b", row("alpha", 1));
    let dio = eager_dio(Vista::new("items", Box::new(shell.clone()))).await?;

    // Appears upstream only — the cache does not have it.
    shell.set_record("c", row("beta", 2));

    let mut ordered = dio.vista();
    ordered.add_order("name", SortDirection::Ascending)?;
    assert_eq!(
        names(&ordered.list_values().await?),
        vec!["alpha", "beta", "gamma"],
        "a pushed-down order is answered over the master's whole set",
    );

    assert_eq!(
        dio.vista().list_values().await?.len(),
        2,
        "while an unnarrowed read still comes from the cache",
    );
    Ok(())
}

// ---- independence -----------------------------------------------------------

/// Narrowing is per-handle. Two facades over one Dio must not disturb each
/// other — the shared-mutation trap an in-place setter falls into.
#[tokio::test]
async fn two_facades_narrow_independently() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let mut ascending = dio.vista();
    ascending.add_order("name", SortDirection::Ascending)?;
    let mut descending = dio.vista();
    descending.add_order("name", SortDirection::Descending)?;

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
        dio.vista().list_values().await?.len(),
        4,
        "a fresh facade is unnarrowed"
    );
    Ok(())
}

/// `clone_shell` carries the narrowing but stays independent.
#[tokio::test]
async fn a_cloned_facade_inherits_but_does_not_share() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let mut base = dio.vista();
    base.add_condition_eq("price", CborValue::Integer(5.into()))?;
    let mut cloned = Vista::new(
        "products",
        base.source.clone_shell().expect("dio shells clone"),
    );
    cloned.add_order("name", SortDirection::Descending)?;

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
    let dio = eager_dio(Vista::new("items", Box::new(shell.clone()))).await?;

    shell.set_record("z", row("zeta", 9));
    assert_eq!(
        dio.vista().list_values().await?.len(),
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
    let dio = eager_dio(Vista::new("items", Box::new(shell.clone()))).await?;

    let mut narrowed = dio.vista();
    narrowed.add_condition_eq("name", CborValue::Text("gamma".into()))?;
    narrowed.add_order("price", SortDirection::Descending)?;

    let preview = narrowed.preview_query();
    let master = preview["master"].to_string();
    assert!(
        master.contains("gamma"),
        "the master accepted the filter, so it must appear there: {preview}",
    );
    assert_eq!(
        preview["facade"]["conditions"],
        serde_json::json!([]),
        "nothing was refused, so nothing is applied over the cache: {preview}",
    );
    assert_eq!(preview["facade"]["order"], serde_json::Value::Null);
    Ok(())
}

/// The mirror case. CSV can neither order nor filter here, so every clause is
/// answered over the cache and the master query stays bare.
#[tokio::test]
async fn preview_reports_a_refused_order_on_the_facade() -> Result<()> {
    let dio = eager_dio(csv_master()?).await?;

    let mut narrowed = dio.vista();
    narrowed.add_order("name", SortDirection::Ascending)?;

    let preview = narrowed.preview_query();
    assert_eq!(
        preview["facade"]["order"],
        serde_json::json!("name asc"),
        "CSV refused the order, so it is applied locally: {preview}",
    );
    assert_eq!(preview["driver"], serde_json::json!("dio"));
    Ok(())
}
