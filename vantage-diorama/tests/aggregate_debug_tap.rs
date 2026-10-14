//! The aggregate engine has no datasource of its own — its debug lines are
//! inherited from whatever tap the *source* Dio was built with.

mod support;
use support::aggregate::{
    int, master, record, revenue, settle, source, source_with_debug,
    source_with_debug_and_failing_refresh, text,
};
use support::capture::{capture, lines_containing};
use vantage_core::Result;
use vantage_diorama::aggregate::{GroupBy, Sum};

#[tokio::test]
async fn aggregate_recompute_line_is_inherited_from_the_source_tap() -> Result<()> {
    let (_guard, log) = capture();

    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(5))]));

    let src = source_with_debug(shell.clone(), "agg-ds").await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;
    settle().await;

    let lines = lines_containing(&log, "recomputed from");
    assert!(!lines.is_empty(), "expected at least one recompute line");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("rows \u{2192}") && l.contains("published")),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|l| l.contains("agg-ds")),
        "every inherited line must carry the source's ds: {lines:?}"
    );
    assert!(
        lines.iter().all(|l| l.contains("\"items#aggregate\"")),
        "{lines:?}"
    );

    drop(derived);
    Ok(())
}

#[tokio::test]
async fn a_non_debug_source_emits_no_aggregate_recompute_lines() -> Result<()> {
    let (_guard, log) = capture();

    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );

    let src = source(shell.clone()).await;
    let derived = src.aggregate(GroupBy::column("status", revenue())).await?;
    settle().await;

    // A source refresh too — recompute definitely runs at least once more.
    shell.set_record("c", record(&[("status", text("open")), ("amount", int(3))]));
    src.refresh().await?;
    settle().await;

    assert!(
        lines_containing(&log, "recomputed from").is_empty(),
        "a non-debug source must produce zero lines: {:?}",
        log.lock().unwrap()
    );

    drop(derived);
    Ok(())
}

/// A value scenery's `refresh_source` covers a source with no refresh route
/// by nudging the engine loop directly, in addition to asking the source to
/// refresh. A working refresh would also emit `DatasetChanged`, racing the
/// nudge for which one the engine loop's `wait()` observes first — so this
/// test's source fails its `on_refresh` (see
/// `support::source_with_debug_and_failing_refresh`), which suppresses that
/// `DatasetChanged` and leaves the nudge as the only thing that wakes the
/// loop, deterministically.
#[tokio::test]
async fn nudge_trigger_is_emitted_by_a_values_local_write_through() -> Result<()> {
    let (_guard, log) = capture();

    let shell = master(&[("amount", "i64")]);
    shell.set_record("a", record(&[("amount", int(10))]));

    let src = source_with_debug_and_failing_refresh(shell.clone(), "agg-ds").await;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
    settle().await;
    // Only interested in what refresh_source triggers below.
    log.lock().unwrap().clear();

    total.refresh_source();
    settle().await;

    let nudge_lines = lines_containing(&log, "(nudge,");
    assert!(
        !nudge_lines.is_empty(),
        "expected a recompute line triggered by the local nudge: {:?}",
        log.lock().unwrap()
    );
    assert!(
        nudge_lines
            .iter()
            .all(|l| l.contains("recomputed from") && l.contains("\"items.value\"")),
        "{nudge_lines:?}"
    );
    // The failing refresh must not have smuggled in a DatasetChanged-
    // triggered line alongside the nudge — the whole point of the failing
    // source is to isolate the one trigger.
    assert!(
        lines_containing(&log, "(DatasetChanged,").is_empty(),
        "the refresh was made to fail precisely so it wouldn't race the nudge: {:?}",
        log.lock().unwrap()
    );

    drop(total);
    Ok(())
}
