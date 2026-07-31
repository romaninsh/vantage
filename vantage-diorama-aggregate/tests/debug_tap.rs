//! The aggregate engine has no datasource of its own — its debug lines are
//! inherited from whatever tap the *source* Dio was built with.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};

mod support;
use ciborium::Value as CborValue;
use support::{
    DEBOUNCE, int, master, record, settle, source, source_with_debug,
    source_with_debug_and_failing_refresh, text,
};
use vantage_core::Result;
use vantage_diorama_aggregate::{AggregateLens, GroupBy, Reduce, Sum};
use vantage_types::Record;
use vantage_vista::Column;

/// Collects every `vantage_diorama::debug` event as one flat string:
/// `"<message> ds=<..> <field>=<..> ..."` — order of fields as emitted.
///
/// Copied from `vantage-diorama/tests/debug_tap.rs` — crates don't share
/// test helpers.
struct CaptureLayer(Arc<Mutex<Vec<String>>>);

struct FlatVisitor {
    message: String,
    fields: String,
}

impl Visit for FlatVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "vantage_diorama::debug" {
            return;
        }
        let mut v = FlatVisitor {
            message: String::new(),
            fields: String::new(),
        };
        event.record(&mut v);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", v.message, v.fields));
    }
}

/// Install a thread-local subscriber that captures every
/// `vantage_diorama::debug` event as a flat string. Returns the guard
/// (drop it to uninstall) and the shared log the events land in.
fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(log.clone()));
    (tracing::subscriber::set_default(subscriber), log)
}

/// Captured lines whose flattened text contains `needle`.
fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

/// The revenue-per-status reducer used by the grouping tests in
/// `aggregate.rs` — copied here rather than shared, same reasoning as the
/// capture harness.
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
    let derived = AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()?
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;
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
        lines.iter().all(|l| l.contains("\"by_status\"")),
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
    let derived = AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()?
        .derive(&src, "by_status", GroupBy::column("status", revenue()))
        .await?;
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

/// `AggregateValue::request_refresh` covers a source with no refresh route
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
    let total = AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()?
        .value(&src, "total", Sum::new("amount"))
        .await?;
    settle().await;
    // Only interested in what request_refresh triggers below.
    log.lock().unwrap().clear();

    total.request_refresh();
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
            .all(|l| l.contains("recomputed from") && l.contains("\"total\"")),
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
