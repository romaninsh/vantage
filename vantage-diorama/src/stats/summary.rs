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
