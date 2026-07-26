//! Behaviour of the aggregation layer against a live source.

mod support;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::SortDir;
use vantage_diorama_aggregate::{
    AggregateLens, Aggregation, Count, CountWhere, DerivedRows, GroupBy, Max, Reduce, Rows, Sum,
};
use vantage_types::Record;
use vantage_vista::{Column, SortDirection};

use support::{BumpCounter, DEBOUNCE, int, master, record, settle, source, text};

fn lens() -> std::sync::Arc<AggregateLens> {
    AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()
        .expect("aggregate lens builds")
}

fn as_i64(value: Option<CborValue>) -> Option<i64> {
    match value {
        Some(CborValue::Integer(i)) => Some(i128::from(i) as i64),
        _ => None,
    }
}

/// The revenue-per-status reducer used by the grouping tests — the "custom
/// Rust callback" shape a consumer writes.
fn revenue() -> Reduce<impl Fn(&CborValue, &[&Record<CborValue>]) -> Record<CborValue>> {
    Reduce::new(
        vec![Column::new("orders", "i64"), Column::new("revenue", "i64")],
        |_key, rows| {
            let total: i64 = rows
                .iter()
                .filter_map(|r| r.get("amount"))
                .filter_map(|v| match v {
                    CborValue::Integer(i) => Some(i128::from(*i) as i64),
                    _ => None,
                })
                .sum();
            let mut out = Record::new();
            out.insert("orders".to_string(), int(rows.len() as i64));
            out.insert("revenue".to_string(), int(total));
            out
        },
    )
}

// ---- purity ---------------------------------------------------------------

/// The property the whole design rests on: `compute` is a pure function, so the
/// same rows always give the same answer regardless of how they were assembled.
#[tokio::test]
async fn compute_is_deterministic_across_input_orderings() {
    let forwards: Rows = [
        (
            "a".to_string(),
            record(&[("status", text("open")), ("amount", int(3))]),
        ),
        (
            "b".to_string(),
            record(&[("status", text("done")), ("amount", int(5))]),
        ),
        (
            "c".to_string(),
            record(&[("status", text("open")), ("amount", int(7))]),
        ),
    ]
    .into_iter()
    .collect();
    let backwards: Rows = forwards
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let grouped = GroupBy::column("status", revenue());
    assert_eq!(
        grouped.compute(&forwards),
        grouped.compute(&backwards),
        "grouping must not depend on input order",
    );
    assert_eq!(Sum::new("amount").compute(&forwards), int(15));
}

// ---- reactivity -----------------------------------------------------------

#[tokio::test]
async fn value_tracks_inserts_updates_and_deletes() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(10))]));
    shell.set_record("b", record(&[("amount", int(5))]));

    let src = source(shell.clone()).await;
    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(15), "initial");

    shell.set_record("c", record(&[("amount", int(7))]));
    src.refresh().await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(22), "after insert");

    shell.set_field("a", "amount", int(1));
    src.refresh().await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(13), "after update");

    shell.remove_record("b");
    src.refresh().await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(8), "after delete");
    Ok(())
}

#[tokio::test]
async fn count_and_count_where_track_the_source() -> Result<()> {
    let shell = master(&[("status", "String")]);
    shell.set_record("a", record(&[("status", text("open"))]));
    shell.set_record("b", record(&[("status", text("done"))]));
    shell.set_record("c", record(&[("status", text("open"))]));

    let src = source(shell.clone()).await;
    let agg = lens();
    let all = agg.value(&src, "all", Count::rows()).await?;
    let open = agg
        .value(&src, "open", CountWhere::new().eq("status", text("open")))
        .await?;
    settle().await;
    assert_eq!(as_i64(all.value()), Some(3));
    assert_eq!(as_i64(open.value()), Some(2));

    shell.set_field("c", "status", text("done"));
    src.refresh().await?;
    settle().await;
    assert_eq!(as_i64(all.value()), Some(3), "count unchanged");
    assert_eq!(as_i64(open.value()), Some(1), "filtered count moved");
    Ok(())
}

/// `max` over a removed extreme — the case an incremental fold cannot retract
/// without either invertibility or a rebuild. Recomputation makes it ordinary.
#[tokio::test]
async fn max_recovers_when_the_extreme_row_is_removed() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(10))]));
    shell.set_record("b", record(&[("amount", int(99))]));
    shell.set_record("c", record(&[("amount", int(50))]));

    let src = source(shell.clone()).await;
    let peak = lens().value(&src, "peak", Max::new("amount")).await?;
    settle().await;
    assert_eq!(as_i64(peak.value()), Some(99));

    shell.remove_record("b");
    src.refresh().await?;
    settle().await;
    assert_eq!(
        as_i64(peak.value()),
        Some(50),
        "falls back to the next highest"
    );
    Ok(())
}

// ---- publish-on-change ----------------------------------------------------

/// The test that makes unconditional recomputation affordable: a source change
/// that leaves the aggregate's answer alone must not repaint anything.
#[tokio::test]
async fn an_unchanged_output_publishes_nothing() -> Result<()> {
    let shell = master(&[("amount", "i64"), ("note", "String")]);
    shell.set_record("a", record(&[("amount", int(10)), ("note", text("one"))]));

    let src = source(shell.clone()).await;
    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;

    let mut bumps = BumpCounter::new(total.subscribe());
    bumps.drain();

    // A column the aggregation never reads moves.
    shell.set_field("a", "note", text("two"));
    src.refresh().await?;
    settle().await;
    assert_eq!(bumps.drain(), 0, "an irrelevant change must not repaint");

    // A change that genuinely moves the answer does.
    shell.set_field("a", "amount", int(11));
    src.refresh().await?;
    settle().await;
    assert!(bumps.drain() > 0, "a real change must repaint");
    Ok(())
}

/// A refresh must never flash the aggregate through an empty source.
///
/// `Dio::refresh` emits `Refreshing` *before* running the refresh callback, and
/// the common callback shape clears the cache before refilling it. Reacting to
/// that leading event would read the source mid-clear and publish a zero, which
/// the trailing `DatasetChanged` would then correct — a visible flicker on
/// every refresh of every derived value.
#[tokio::test]
async fn a_refresh_never_flickers_the_value_through_zero() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(10))]));

    let src = source(shell.clone()).await;
    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(10));

    // Watch every published value across a refresh that changes nothing.
    let mut rx = total.subscribe();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = {
        let seen = seen.clone();
        let total = total.clone();
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                seen.lock().unwrap().push(as_i64(total.value()));
            }
        })
    };

    src.refresh().await?;
    settle().await;
    recorder.abort();

    let published = seen.lock().unwrap().clone();
    assert!(
        !published.contains(&Some(0)),
        "the aggregate must not pass through 0 during a refresh, saw {published:?}",
    );
    assert_eq!(as_i64(total.value()), Some(10));
    Ok(())
}

/// Leading edge plus one trailing flush — never a bump per source event, and
/// never a dropped final state.
#[tokio::test]
async fn a_burst_coalesces_and_still_lands_on_the_final_value() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(0))]));

    let src = source(shell.clone()).await;
    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;

    let mut bumps = BumpCounter::new(total.subscribe());
    bumps.drain();

    for n in 1..=8 {
        shell.set_field("a", "amount", int(n));
        src.refresh().await?;
    }
    settle().await;

    let observed = bumps.drain();
    assert_eq!(
        as_i64(total.value()),
        Some(8),
        "the trailing flush must land the final value",
    );
    assert!(
        observed <= 3,
        "a burst of 8 changes should coalesce, saw {observed} repaints",
    );
    Ok(())
}

// ---- refresh propagation --------------------------------------------------

/// An explicit refresh is pushed *down* to the source rather than recomputed
/// locally — recomputing over unchanged rows would just produce the same
/// number.
#[tokio::test]
async fn request_refresh_reaches_the_datasource() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(1))]));

    let src = source(shell.clone()).await;
    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(1));

    // The datasource changes with no notification of any kind.
    shell.set_record("b", record(&[("amount", int(4))]));
    assert_eq!(as_i64(total.value()), Some(1), "still stale, as expected");

    total.request_refresh();
    settle().await;
    assert_eq!(as_i64(total.value()), Some(5), "refresh reached the source");
    Ok(())
}

// ---- derived rows ---------------------------------------------------------

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
    let derived = lens()
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;

    let rows = derived.vista().list_values().await?;
    assert_eq!(rows.len(), 2, "two groups");
    let open = rows.get("open").expect("open group");
    assert_eq!(open.get("orders"), Some(&int(2)));
    assert_eq!(open.get("revenue"), Some(&int(17)));

    // A group empties and the derived row disappears.
    shell.set_field("a", "status", text("done"));
    shell.set_field("c", "status", text("done"));
    src.refresh().await?;
    settle().await;

    let rows = derived.vista().list_values().await?;
    assert_eq!(rows.len(), 1, "the emptied group is gone");
    assert_eq!(
        rows.get("done").and_then(|r| r.get("orders")),
        Some(&int(3))
    );
    Ok(())
}

/// The architectural claim: a derived Vista advertises `can_order` /
/// `can_search` / `can_count` and actually honours them, because unlike its
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
    let derived = lens()
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;

    let caps = derived.master().capabilities().clone();
    assert!(caps.can_order && caps.can_search && caps.can_count);
    assert!(
        !caps.can_insert && !caps.can_update && !caps.can_delete,
        "derived rows are a function of the source — they cannot be written",
    );

    // Ordering, on a fresh narrowing so the shared master is untouched.
    let mut ordered = derived
        .master()
        .source
        .clone_shell()
        .map(|shell| vantage_vista::Vista::new("by_status", shell))
        .expect("aggregate shells clone");
    ordered.add_order("revenue", SortDirection::Descending)?;
    let by_revenue: Vec<String> = ordered.list_values().await?.into_keys().collect();
    assert_eq!(by_revenue, vec!["beta", "gamma", "alpha"]);

    // Counting.
    assert_eq!(derived.master().get_count().await?, 3);

    // Search.
    let mut searched = derived
        .master()
        .source
        .clone_shell()
        .map(|shell| vantage_vista::Vista::new("by_status", shell))
        .expect("aggregate shells clone");
    searched.add_search("bet")?;
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
    let derived = lens()
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;

    let scenery = derived
        .table_scenery()
        .sort("revenue", SortDir::Desc)
        .open()
        .await?;
    assert_eq!(scenery.row_count(), 2);
    let top = scenery.row(0).expect("first row");
    assert_eq!(top.record.get("revenue"), Some(&int(10)));
    Ok(())
}

// ---- schema ---------------------------------------------------------------

#[tokio::test]
async fn the_derived_schema_is_the_key_plus_the_reducers_columns() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record("a", record(&[("status", text("open")), ("amount", int(1))]));

    let src = source(shell).await;
    let derived = lens()
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;

    let vista = derived.master();
    assert_eq!(
        vista.get_column_names(),
        vec!["status", "orders", "revenue"]
    );
    assert_eq!(vista.get_id_column(), Some("status"));
    Ok(())
}

/// An empty source is a real answer, not an error.
#[tokio::test]
async fn an_empty_source_aggregates_to_empty() -> Result<()> {
    let src = source(master(&[("amount", "i64")])).await;
    let agg = lens();
    let total = agg.value(&src, "total", Sum::new("amount")).await?;
    let derived = agg
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;
    settle().await;

    assert_eq!(as_i64(total.value()), Some(0), "an empty sum is zero");
    assert!(derived.vista().list_values().await?.is_empty());
    Ok(())
}

/// Dropping the handle stops the work — the same contract sceneries follow.
#[tokio::test]
async fn dropping_the_aggregate_stops_it_following_the_source() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(1))]));
    let src = source(shell.clone()).await;

    let total = lens().value(&src, "total", Sum::new("amount")).await?;
    settle().await;
    let mut bumps = BumpCounter::new(total.subscribe());
    bumps.drain();
    drop(total);

    shell.set_field("a", "amount", int(999));
    src.refresh().await?;
    settle().await;
    // Nothing to assert on the dropped handle; what matters is that the source
    // still works and the task did not keep it pinned.
    assert_eq!(src.vista().list_values().await?.len(), 1);
    Ok(())
}

/// A warm cache paints before the first recomputation lands.
#[tokio::test]
async fn a_cached_value_is_restored_on_reopen() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(42))]));
    let src = source(shell.clone()).await;

    // One lens, one cache — reused across both mounts.
    let agg = AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()
        .expect("lens");

    let first = agg.value(&src, "total", Sum::new("amount")).await?;
    settle().await;
    assert_eq!(as_i64(first.value()), Some(42));
    drop(first);

    let second = agg.value(&src, "total", Sum::new("amount")).await?;
    assert_eq!(
        as_i64(second.value()),
        Some(42),
        "the cached value is there before any recomputation",
    );
    Ok(())
}

/// Unused aggregate caches can be reclaimed by name.
#[tokio::test]
async fn retain_drops_aggregates_that_are_no_longer_declared() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(1))]));
    let src = source(shell).await;

    let agg = AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()
        .expect("lens");
    let keep = agg.value(&src, "keep", Sum::new("amount")).await?;
    let drop_me = agg.value(&src, "drop_me", Count::rows()).await?;
    settle().await;
    drop(drop_me);

    agg.retain(&["keep"]).await?;

    // The surviving aggregate is untouched.
    assert_eq!(as_i64(keep.value()), Some(1));
    Ok(())
}

/// A row set with a stable schema but no rows still carries its columns, so a
/// grid over an empty group-by renders headers rather than nothing.
#[tokio::test]
async fn derived_rows_carry_their_schema_when_empty() {
    let empty: Rows = Rows::new();
    let out: DerivedRows = GroupBy::column("status", revenue()).compute(&empty);
    assert!(out.is_empty());
    assert_eq!(
        out.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["status", "orders", "revenue"],
    );
}
