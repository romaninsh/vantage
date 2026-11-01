//! Derived Dios: `dio.aggregate(..)` over a live source.

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::{AsRows, Count, GroupBy, Sum};
use vantage_vista::{Capability, SortDirection, Vista};

use crate::support::aggregate::{as_i64, int, master, record, revenue, settle, source, text};

#[tokio::test]
async fn group_by_produces_a_queryable_derived_dio() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(5))]));
    shell.set_record("c", record(&[("status", text("open")), ("amount", int(7))]));

    let src = source(shell.clone()).await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;

    let rows = derived.cached().list_values().await?;
    assert_eq!(rows.len(), 2, "two groups");
    let open = rows.get("open").expect("open group");
    assert_eq!(open.get("orders"), Some(&int(2)));
    assert_eq!(open.get("revenue"), Some(&int(17)));

    // A group empties and the derived row disappears.
    shell.set_field("a", "status", text("done"));
    shell.set_field("c", "status", text("done"));
    src.refresh().await?;
    settle().await;

    let rows = derived.cached().list_values().await?;
    assert_eq!(rows.len(), 1, "the emptied group is gone");
    assert_eq!(
        rows.get("done").and_then(|r| r.get("orders")),
        Some(&int(3))
    );
    Ok(())
}

/// The architectural claim: a derived Vista advertises `Order` and `Search`
/// (and counts) and actually honours them, because unlike its
/// source it holds its whole row set.
#[tokio::test]
async fn the_derived_vista_honours_the_capabilities_it_advertises() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("alpha")), ("amount", int(1))]),
    );
    shell.set_record(
        "b",
        record(&[("status", text("beta")), ("amount", int(50))]),
    );
    shell.set_record(
        "c",
        record(&[("status", text("gamma")), ("amount", int(7))]),
    );

    let src = source(shell.clone()).await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;

    let caps = derived.master().capabilities();
    assert!(caps.has(Capability::Order) && caps.has(Capability::Search));
    assert!(
        !caps.has(Capability::Insert)
            && !caps.has(Capability::Update)
            && !caps.has(Capability::Delete),
        "derived rows are a function of the source — they cannot be written",
    );

    // Ordering, on a fresh narrowing so the shared master is untouched.
    let ordered =
        Vista::clone(derived.master()).with_order("revenue", SortDirection::Descending)?;
    let by_revenue: Vec<String> = ordered.list_values().await?.into_keys().collect();
    assert_eq!(by_revenue, vec!["beta", "gamma", "alpha"]);

    // Counting.
    assert_eq!(derived.master().count().await?, 3);

    // Search.
    let searched = Vista::clone(derived.master()).with_search("bet")?;
    let hits: Vec<String> = searched.list_values().await?.into_keys().collect();
    assert_eq!(hits, vec!["beta"]);
    Ok(())
}

/// The derived Dio is an ordinary Dio, so a grid binds to it the usual way.
#[tokio::test]
async fn a_scenery_opens_over_the_derived_dio() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(5))]));

    let src = source(shell.clone()).await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;

    let scenery = derived
        .table_scenery()
        .order("revenue", SortDirection::Descending)
        .open()
        .await?;
    assert_eq!(scenery.row_count(), 2);
    let top = scenery.row(0).expect("first row");
    assert_eq!(top.record.get("revenue"), Some(&int(10)));
    Ok(())
}

#[tokio::test]
async fn the_derived_schema_is_the_key_plus_the_reducers_columns() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record("a", record(&[("status", text("open")), ("amount", int(1))]));

    let src = source(shell).await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;

    let vista = derived.master();
    assert_eq!(
        vista
            .columns()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["status", "orders", "revenue"]
    );
    assert_eq!(vista.id_column(), Some("status"));
    Ok(())
}

/// An empty source is a real answer, not an error.
#[tokio::test]
async fn an_empty_source_aggregates_to_empty() -> Result<()> {
    let src = source(master(&[("amount", "i64")])).await;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;
    settle().await;

    assert_eq!(as_i64(total.value()), Some(0), "an empty sum is zero");
    assert!(derived.cached().list_values().await?.is_empty());
    Ok(())
}

/// A scalar mounted through `AsRows` is a table with one row and one column,
/// the shape SQL gives a `count(*)`.
#[tokio::test]
async fn an_as_rows_aggregate_is_a_one_row_dio() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    for (id, amount) in [("a", 1), ("b", 2), ("c", 3)] {
        shell.set_record(id, record(&[("amount", int(amount))]));
    }
    let src = source(shell).await;
    let count = src.aggregate(AsRows::new("n", Count::rows())).await?;

    let rows = count.cached().list_values().await?;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = rows.values().next().expect("the one row");
    assert_eq!(row.get("n"), Some(&int(3)));
    assert_eq!(row.len(), 1, "one column: {row:?}");
    Ok(())
}

/// Every aggregate of one source has the same vista name, and each is its own
/// Dio with its own rows: one never hands back another's. Dropping one leaves
/// the other following the source.
#[tokio::test]
async fn two_aggregates_of_one_source_are_separate_dios() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record("a", record(&[("status", text("open")), ("amount", int(1))]));
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(2))]));
    let src = source(shell.clone()).await;

    let groups = src.aggregate(GroupBy::column("status", revenue())).await?;
    let count = src.aggregate(AsRows::new("n", Count::rows())).await?;
    assert_eq!(groups.master().name(), count.master().name());

    let group_ids: Vec<String> = groups.cached().list_values().await?.into_keys().collect();
    assert_eq!(group_ids.len(), 2, "{group_ids:?}");
    let counted = count.cached().list_values().await?;
    assert_eq!(counted.len(), 1, "{counted:?}");
    assert_eq!(
        counted.values().next().and_then(|r| r.get("n")),
        Some(&int(2))
    );

    drop(groups);
    shell.set_record("c", record(&[("status", text("held")), ("amount", int(3))]));
    src.refresh().await?;
    settle().await;
    let counted = count.cached().list_values().await?;
    assert_eq!(
        counted.values().next().and_then(|r| r.get("n")),
        Some(&int(3))
    );
    Ok(())
}

/// The returned Dio is the handle: nothing else keeps it, and once its last
/// clone drops the recomputation stops and releases the source.
#[tokio::test]
async fn dropping_the_aggregate_stops_it_following_the_source() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record("a", record(&[("status", text("open")), ("amount", int(1))]));
    let src = source(shell.clone()).await;

    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;
    settle().await;
    let derived_weak = derived.downgrade();
    let source_weak = src.downgrade();
    drop(derived);
    settle().await;
    assert!(
        derived_weak.upgrade().is_none(),
        "only the returned handle keeps the derived Dio"
    );

    shell.set_field("a", "amount", int(999));
    src.refresh().await?;
    settle().await;
    assert!(
        derived_weak.upgrade().is_none(),
        "a source refresh doesn't bring the dropped aggregate back"
    );

    drop(src);
    settle().await;
    assert!(
        source_weak.upgrade().is_none(),
        "a stopped aggregate no longer holds its source"
    );
    Ok(())
}
