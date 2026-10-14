//! The fetch ledger: every window a table pulled from its master.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

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
#[non_exhaustive]
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
    ranges: HashMap<String, usize>,
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

static LEDGER: LazyLock<Mutex<HashMap<String, TableLedger>>> = LazyLock::new(Default::default);

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
