//! The lines a chunk load emits as it dispatches and commits, and the
//! formatting behind them. Kept out of
//! [`run_chunk_callback`](super::dispatch::run_chunk_callback) and
//! [`finish_chunk_load`](super::commit::finish_chunk_load) so both read as
//! bookkeeping rather than as string building; each tap helper re-checks the
//! tap, so an off tap costs a comparison.

use std::ops::Range;

use vantage_core::Priority;
use vantage_vista::VistaQuery;

use crate::debug::DebugTap;
use crate::dio::DioInner;
use crate::lens::chunk_sink::FlushReport;
use crate::scenery::table::state::SharedRows;

/// A fetch about to be dispatched, as its lines report it.
pub(super) struct Dispatch<'a> {
    pub(super) req: Option<u64>,
    pub(super) visible: &'a Range<usize>,
    pub(super) visible_cached: usize,
    pub(super) effective: &'a Range<usize>,
    pub(super) effective_cached: usize,
    pub(super) force_load: bool,
    pub(super) priority: Priority,
    pub(super) query: &'a VistaQuery,
}

/// The tracing lines and the "fetch asks for" tap line of a dispatched
/// fetch: the moment a range costs a round trip.
pub(super) fn trace_dispatch(tap: &DebugTap, dio_inner: &DioInner, dispatch: Dispatch) {
    let Dispatch {
        req,
        visible,
        visible_cached,
        effective,
        effective_cached,
        force_load,
        priority,
        query,
    } = dispatch;
    let visible_len = visible.end.saturating_sub(visible.start);
    let effective_len = effective.len();
    let effective_to_fetch = effective_len - effective_cached;
    tracing::debug!(
        target: "vantage_diorama::viewport",
        visible = ?visible,
        visible_len,
        visible_cached,
        effective = ?effective,
        effective_len,
        effective_cached,
        effective_to_fetch,
        effective_overfetch_pct = if effective_len > 0 {
            (effective_cached as f64 / effective_len as f64) * 100.0
        } else {
            0.0
        },
        force_load,
        priority = ?priority,
        "fire_chunk_load: dispatching the paged loader",
    );
    // The counterpart of the CACHE line `choose_window` emits: this is the
    // moment a range costs a round trip. `already_cached` says how much of
    // what we are about to fetch the cache ALREADY holds — non-zero means the
    // fetch is re-reading rows we have, which on a `force_load` is by design
    // and otherwise is worth explaining.
    tracing::debug!(
        target: "vantage_diorama::source",
        table = %dio_inner.master.name(),
        range = ?effective,
        rows = effective_to_fetch,
        already_cached = effective_cached,
        force_load,
        "MASTER — fetching from the datasource",
    );
    crate::debug::tapline!(
        tap,
        "dio",
        "fetch #{} asks for rows {}..{}{} — {} missing, {} already held{}{}",
        req.unwrap_or_default(),
        effective.start,
        effective.end,
        if effective == visible {
            String::new()
        } else {
            format!(" (viewport {}..{})", visible.start, visible.end)
        },
        effective_to_fetch,
        effective_cached,
        match &query.order {
            Some((col, dir)) => format!(" · sorted by {col} {dir:?}"),
            None => String::new(),
        },
        match &query.search {
            Some(q) => format!(" · searching \"{q}\""),
            None => String::new(),
        },
    );
}

/// "cache write": what the flush just committed, split into rows that were
/// genuinely new vs. rows that already had a cached value and got overwritten,
/// plus how much of the known total is now cached.
///
/// Says nothing without a report (the load failed before the flush), and
/// nothing when the page came back empty: `flush_counted` short-circuits
/// before counting, so `cache_rows_after` would read `0` — not the cache's
/// real size, just "not measured" — and a line for zero rows written is not a
/// write at all.
pub(super) fn tap_cache_write(tap: &DebugTap, state: &SharedRows, report: Option<&FlushReport>) {
    let Some(report) = report else {
        return;
    };
    if !tap.enabled() || report.written == 0 {
        return;
    }
    // A concurrent flush from another in-flight load between the before/after
    // `count()` calls could in principle make this go negative and underflow;
    // accepted here since it's a debug-only line, not a value anything
    // downstream trusts.
    let new_rows = (report.cache_rows_after - report.cache_rows_before) as usize;
    let held = report.cache_rows_after as usize;
    let known_total = state.total.read().unwrap().unwrap_or(0);
    crate::debug::tapline!(
        tap,
        "cache",
        "+{} new, {} updated — now holding {} of {} ({})",
        new_rows,
        report.written.saturating_sub(new_rows),
        crate::debug::num(held),
        crate::debug::num(known_total),
        crate::debug::pct(held, known_total),
    );
}

/// "payload": the wide-data detector — how many distinct fields this chunk
/// carried against how many the open sceneries actually asked for, and the
/// encoded weight of the page.
///
/// `demanded_columns` does a live scan of the table sceneries, so the whole
/// block is worth paying for only when the tap is on — the same reasoning
/// that gates the encoding itself in `ChunkSink::flush_counted`.
pub(super) fn tap_payload_columns(
    tap: &DebugTap,
    dio_inner: &DioInner,
    state: &SharedRows,
    flush_report: Option<&FlushReport>,
) {
    if !tap.enabled() {
        return;
    }
    let demanded = dio_inner.demanded_columns();
    let received: &[String] = flush_report
        .map(|r| r.columns_received.as_slice())
        .unwrap_or(&[]);
    let payload_bytes = flush_report.map(|r| r.payload_bytes).unwrap_or(0);
    let received_count = received.len();
    const SAMPLE: usize = 8;
    let received_sample = if received_count > SAMPLE {
        format!(
            "{} +{} more",
            received[..SAMPLE].join(","),
            received_count - SAMPLE
        )
    } else {
        received.join(",")
    };
    let undemanded_count = match &demanded {
        None => 0,
        Some(set) => received.iter().filter(|c| !set.contains(*c)).count(),
    };
    // Name the columns the first time only. After that the set is established
    // and repeating it every fetch is just width.
    let first_time = !state
        .payload_named
        .swap(true, std::sync::atomic::Ordering::Relaxed);
    // With no demand declared, "all columns are wanted" is an assumption, not
    // a fact — saying "206/206 displayed" would claim a grid showing five
    // columns wanted all two hundred. Report what is actually known instead.
    let headline = match &demanded {
        None => format!(
            "{} columns received · no view declared what it needs, so all were fetched",
            received_count
        ),
        Some(_) => format!(
            "{} of {} columns wanted · {} fetched and dropped",
            received_count.saturating_sub(undemanded_count),
            received_count,
            undemanded_count
        ),
    };
    if first_time {
        crate::debug::tapline!(
            tap,
            "payload",
            "{} · {} · {}",
            headline,
            crate::debug::bytes(payload_bytes),
            received_sample,
        );
    } else {
        crate::debug::tapline!(
            tap,
            "payload",
            "{} · {}",
            headline,
            crate::debug::bytes(payload_bytes),
        );
    }
}
