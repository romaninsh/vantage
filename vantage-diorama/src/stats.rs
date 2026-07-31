//! Live-instance counters for leak diagnosis.
//!
//! Each counted type owns a `Tally` field: constructing the type
//! increments its counter, dropping it decrements. [`live_counts`] snapshots
//! all of them — an embedder can log it periodically to verify that closing
//! a page really releases its Dios and sceneries instead of accumulating
//! them.

use std::sync::atomic::{AtomicUsize, Ordering};

static DIOS: AtomicUsize = AtomicUsize::new(0);
static TABLE_SCENERIES: AtomicUsize = AtomicUsize::new(0);
static RECORD_SCENERIES: AtomicUsize = AtomicUsize::new(0);
static SERVOS: AtomicUsize = AtomicUsize::new(0);

/// RAII counter handle — one per counted instance, embedded as a field so
/// every construction/drop path is covered automatically.
#[derive(Debug)]
pub(crate) struct Tally(&'static AtomicUsize);

impl Tally {
    fn claim(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Tally(counter)
    }

    pub(crate) fn dio() -> Self {
        Self::claim(&DIOS)
    }

    pub(crate) fn table_scenery() -> Self {
        Self::claim(&TABLE_SCENERIES)
    }

    pub(crate) fn record_scenery() -> Self {
        Self::claim(&RECORD_SCENERIES)
    }

    pub(crate) fn servo() -> Self {
        Self::claim(&SERVOS)
    }
}

impl Drop for Tally {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Point-in-time census of live diorama objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveCounts {
    pub dios: usize,
    pub table_sceneries: usize,
    pub record_sceneries: usize,
    pub servos: usize,
}

/// Snapshot the live-instance counters.
pub fn live_counts() -> LiveCounts {
    LiveCounts {
        dios: DIOS.load(Ordering::Relaxed),
        table_sceneries: TABLE_SCENERIES.load(Ordering::Relaxed),
        record_sceneries: RECORD_SCENERIES.load(Ordering::Relaxed),
        servos: SERVOS.load(Ordering::Relaxed),
    }
}

// ---- Fetch ledger ----------------------------------------------------------

/// Every window a table pulled from its master, and what it cost.
///
/// A remote fetch is the most expensive thing a grid does and the easiest to do
/// by accident: a range re-requested because the rows never landed, a viewport
/// that re-fires on every repaint, a refresh overlapping a scroll. None of it
/// shows up as an error, and in a log it reads as ordinary activity — the give
/// away is only ever a *count*, which no single log line can carry.
///
/// So the counts are kept. `repeats` and `rows_redundant` are the two numbers
/// worth reading: the first says a range was asked for more than once, the
/// second says rows arrived that the cache already held. Both should be near
/// zero on a healthy table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchStats {
    pub table: String,
    /// Windows requested from the master.
    pub fetches: usize,
    /// Distinct ranges among them.
    pub distinct_ranges: usize,
    /// Fetches for a range already fetched before — `fetches - distinct_ranges`.
    pub repeats: usize,
    /// The most re-requested range, and how many times. A range fetched over
    /// and over is a source that will not serve it.
    pub worst_range: Option<(String, usize)>,
    /// Rows the master handed back, across every fetch.
    pub rows_received: usize,
    /// Of the rows requested, how many the cache already held — work paid for
    /// twice. A forced refresh counts here legitimately; a scroll should not.
    pub rows_redundant: usize,
    pub ms_total: u64,
    pub ms_max: u64,
}

#[derive(Default)]
struct TableLedger {
    fetches: usize,
    ranges: std::collections::HashMap<String, usize>,
    rows_received: usize,
    rows_redundant: usize,
    ms_total: u64,
    ms_max: u64,
}

/// Cap on distinct ranges remembered per table. A grid scrolled through a large
/// set would otherwise grow this map without bound; past the cap the counts
/// stay exact and only the per-range breakdown stops taking new entries — which
/// is the right trade, since a repeat loop repeats a range it already recorded.
const MAX_TRACKED_RANGES: usize = 256;

static LEDGER: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, TableLedger>>,
> = std::sync::LazyLock::new(Default::default);

/// Record one completed master fetch. Called by the chunk loader.
pub(crate) fn record_fetch(
    table: &str,
    range: &std::ops::Range<usize>,
    rows_received: usize,
    rows_redundant: usize,
    ms: u64,
) {
    let Ok(mut ledger) = LEDGER.lock() else {
        return;
    };
    let entry = ledger.entry(table.to_string()).or_default();
    entry.fetches += 1;
    entry.rows_received += rows_received;
    entry.rows_redundant += rows_redundant;
    entry.ms_total += ms;
    entry.ms_max = entry.ms_max.max(ms);
    let key = format!("{}..{}", range.start, range.end);
    let tracked = entry.ranges.len();
    match entry.ranges.get_mut(&key) {
        Some(count) => *count += 1,
        None if tracked < MAX_TRACKED_RANGES => {
            entry.ranges.insert(key, 1);
        }
        None => {}
    }
}

/// Snapshot the fetch ledger, busiest table first.
pub fn fetch_stats() -> Vec<FetchStats> {
    let Ok(ledger) = LEDGER.lock() else {
        return Vec::new();
    };
    let mut out: Vec<FetchStats> = ledger
        .iter()
        .map(|(table, e)| FetchStats {
            table: table.clone(),
            fetches: e.fetches,
            distinct_ranges: e.ranges.len(),
            repeats: e.fetches.saturating_sub(e.ranges.len()),
            worst_range: e
                .ranges
                .iter()
                .max_by_key(|(_, count)| **count)
                .filter(|(_, count)| **count > 1)
                .map(|(range, count)| (range.clone(), *count)),
            rows_received: e.rows_received,
            rows_redundant: e.rows_redundant,
            ms_total: e.ms_total,
            ms_max: e.ms_max,
        })
        .collect();
    out.sort_by(|a, b| b.fetches.cmp(&a.fetches).then(a.table.cmp(&b.table)));
    out
}

/// Forget every recorded fetch — so a measurement can be scoped to one
/// interaction ("open the page, then scroll") instead of the whole session.
pub fn reset_fetch_stats() {
    if let Ok(mut ledger) = LEDGER.lock() {
        ledger.clear();
    }
}

/// Render a session summary block with fetch stats, live counts, and process stats.
/// Returns one string per line, no trailing newlines, in this order:
/// - header: "— diorama session summary —"
/// - per table (busiest first): "{table}: {fetches} fetches ({repeats} repeats), {rows_received} rows ({rows_redundant} redundant), {ms_total}ms total, {ms_max}ms max"
/// - live: "live: {dios} dios, {table_sceneries} table sceneries, {record_sceneries} record sceneries, {servos} servos"
/// - process: "process: uptime {uptime_ms}ms, cpu {cpu_ms}ms, peak rss {peak_rss_mb}MB"
pub fn debug_summary_lines() -> Vec<String> {
    let mut lines = Vec::new();

    // Header
    lines.push("— diorama session summary —".to_string());

    // Per-table stats (already sorted busiest first by fetch_stats())
    for stat in fetch_stats() {
        lines.push(format!(
            "{:<10} {:<8} {} fetches, {} repeats · {} rows, {} redundant · {} waiting, {} slowest",
            stat.table,
            "summary",
            stat.fetches,
            stat.repeats,
            crate::debug::num(stat.rows_received),
            crate::debug::num(stat.rows_redundant),
            crate::debug::dur(stat.ms_total),
            crate::debug::dur(stat.ms_max),
        ));
    }

    // Live counts
    let live = live_counts();
    lines.push(format!(
        "{:<10} {:<8} {} dio, {} table scenery, {} record, {} servo still alive",
        "", "summary", live.dios, live.table_sceneries, live.record_sceneries, live.servos
    ));

    // Process stats
    let proc = crate::debug::process_stats();
    let peak_rss_mb = proc.peak_rss_bytes / (1024 * 1024);
    lines.push(format!(
        "{:<10} {:<8} session {} · cpu {} · peak rss {}MB",
        "",
        "summary",
        crate::debug::dur(proc.uptime_ms),
        crate::debug::dur(proc.cpu_ms),
        peak_rss_mb
    ));

    lines
}

/// Log the debug summary lines via tracing at info level.
/// Unconditional — the embedder decides when to call this.
pub fn emit_debug_summary() {
    // No-op unless some datasource actually opted in, so an embedder can call
    // this unconditionally on the way out without printing a ledger nobody
    // asked for.
    if !crate::debug::any_tap_enabled() {
        return;
    }
    for line in debug_summary_lines() {
        tracing::info!(target: "vantage_diorama::debug", "{}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These two scenarios share the process-global `LEDGER` and each needs
    // to `reset_fetch_stats()` before recording its own fixture. Run as
    // separate `#[test]` fns, cargo's default parallel test threads could
    // interleave the resets and wipe one scenario's records mid-assert —
    // so both live in one test, run strictly in sequence.
    #[test]
    fn repeats_and_waste_are_counted_per_range_then_summary_renders_lines() {
        reset_fetch_stats();
        record_fetch("events", &(0..100), 35, 0, 3000);
        record_fetch("events", &(35..42), 7, 7, 2000);
        record_fetch("events", &(35..42), 7, 7, 2500);
        record_fetch("tags", &(0..50), 50, 0, 10);

        let stats = fetch_stats();
        assert_eq!(stats[0].table, "events", "busiest table first");
        assert_eq!(stats[0].fetches, 3);
        assert_eq!(stats[0].distinct_ranges, 2);
        assert_eq!(stats[0].repeats, 1);
        assert_eq!(
            stats[0].worst_range,
            Some(("35..42".to_string(), 2)),
            "the range that keeps being asked for",
        );
        assert_eq!(stats[0].rows_redundant, 14);
        assert_eq!(stats[0].ms_max, 3000);

        assert_eq!(stats[1].table, "tags");
        assert_eq!(
            stats[1].worst_range, None,
            "fetched once, nothing to report"
        );

        reset_fetch_stats();
        record_fetch("book", &(0..20), 20, 0, 12);
        record_fetch("book", &(0..20), 20, 20, 9);
        let lines = debug_summary_lines();
        assert!(lines[0].contains("session summary"));
        let book = lines.iter().find(|l| l.starts_with("book ")).unwrap();
        assert!(book.contains("2 fetches, 1 repeats"), "{book}");
        assert!(book.contains("20 redundant"), "{book}");
        assert!(lines.iter().any(|l| l.contains("still alive")));
        assert!(lines.iter().any(|l| l.contains("peak rss")));
    }
}
