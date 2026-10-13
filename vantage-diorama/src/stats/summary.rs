//! The end-of-session summary: the fetch ledger, the live counts and what the
//! process cost, rendered as debug-stream lines.

use super::ledger::fetch_stats;
use super::live_counts;
use crate::debug::{dur, num};

/// Render the session summary, one line per element, no trailing newlines, in
/// the debug stream's `<source>  summary  <clause>` shape:
/// - a header, `— diorama session summary —`;
/// - one line per table, busiest first: `<table>  summary  N fetches, N repeats · N rows,
///   N redundant · <time> waiting, <time> slowest`;
/// - the live counts: `summary  N dio, N table scenery, N record, N servo still alive`;
/// - the process: `summary  session <time> · cpu <time> · peak rss NMB`.
pub fn debug_summary_lines() -> Vec<String> {
    let mut lines = vec!["— diorama session summary —".to_string()];

    for stat in fetch_stats() {
        lines.push(format!(
            "{:<10} {:<8} {} fetches, {} repeats · {} rows, {} redundant · {} waiting, {} slowest",
            stat.table,
            "summary",
            stat.fetches,
            stat.repeats,
            num(stat.rows_received),
            num(stat.rows_redundant),
            dur(stat.ms_total),
            dur(stat.ms_max),
        ));
    }

    let live = live_counts();
    lines.push(format!(
        "{:<10} {:<8} {} dio, {} table scenery, {} record, {} servo still alive",
        "", "summary", live.dios, live.table_sceneries, live.record_sceneries, live.servos
    ));

    let proc = crate::debug::process_stats();
    let peak_rss_mb = proc.peak_rss_bytes / (1024 * 1024);
    lines.push(format!(
        "{:<10} {:<8} session {} · cpu {} · peak rss {}MB",
        "",
        "summary",
        dur(proc.uptime_ms),
        dur(proc.cpu_ms),
        peak_rss_mb
    ));

    lines
}

/// Log the summary lines at `info` on the `vantage_diorama::debug` target,
/// once some datasource has enabled the debug stream. Before that it does
/// nothing, so an embedder can call it on the way out without printing a
/// summary nobody asked for.
pub fn emit_debug_summary() {
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
    use crate::stats::ledger::{record_fetch, reset_fetch_stats};

    // These two scenarios share the process-global ledger and each needs
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
