//! Value sceneries following their source: reactivity, publish-on-change and
//! refresh propagation.

use vantage_core::Result;
use vantage_diorama::aggregate::{Conditions, Count, Max, Sum, Where};

use crate::support::aggregate::{
    BumpCounter, as_i64, int, master, record, settle, source, text,
};

#[tokio::test]
async fn value_tracks_inserts_updates_and_deletes() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(10))]));
    shell.set_record("b", record(&[("amount", int(5))]));

    let src = source(shell.clone()).await;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
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
    let all = src.value_scenery(Count::rows()).open().await?;
    let open = src
        .value_scenery(Where::new(
            Conditions::new().eq("status", text("open")),
            Count::rows(),
        ))
        .open()
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
    let peak = src.value_scenery(Max::of("amount")).open().await?;
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

/// The test that makes unconditional recomputation affordable: a source change
/// that leaves the aggregate's answer alone must not repaint anything.
#[tokio::test]
async fn an_unchanged_output_publishes_nothing() -> Result<()> {
    let shell = master(&[("amount", "i64"), ("note", "String")]);
    shell.set_record("a", record(&[("amount", int(10)), ("note", text("one"))]));

    let src = source(shell.clone()).await;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
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
    let total = src.value_scenery(Sum::of("amount")).open().await?;
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
    let total = src.value_scenery(Sum::of("amount")).open().await?;
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

/// An explicit refresh is pushed *down* to the source rather than recomputed
/// locally — recomputing over unchanged rows would just produce the same
/// number.
#[tokio::test]
async fn refresh_source_reaches_the_datasource() -> Result<()> {
    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(1))]));

    let src = source(shell.clone()).await;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
    settle().await;
    assert_eq!(as_i64(total.value()), Some(1));

    // The datasource changes with no notification of any kind.
    shell.set_record("b", record(&[("amount", int(4))]));
    assert_eq!(as_i64(total.value()), Some(1), "still stale, as expected");

    total.refresh_source();
    settle().await;
    assert_eq!(as_i64(total.value()), Some(5), "refresh reached the source");
    Ok(())
}
