//! Dropping the cached rows a whole-set window load no longer lists: rows
//! that left the master with no change to say so (a script's transactional
//! `DELETE`, a change made while the live follow was disconnected).

use std::ops::Range;

use crate::dio::pending::SettleMark;
use crate::dio::{DioEvent, DioInner};
use crate::lens::ChunkSink;
use crate::scenery::table::state::SharedRows;

/// Whether a re-pull of `range` that received `pushed` rows read the
/// view's whole set: it started at the first row and reached the end, by
/// the total it stated or, with none, by coming back short. Read before
/// [`settle_total`](super::totals::settle_total) takes the stated flag.
pub(super) fn read_whole_set(
    state: &SharedRows,
    range: &Range<usize>,
    pushed: usize,
    force_load: bool,
) -> bool {
    if !force_load || range.start != 0 {
        return false;
    }
    if state
        .load_total_reported
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return *state.total.read().unwrap() == Some(pushed);
    }
    pushed < range.len()
}

/// Drop every cached row the load in `writer` didn't list, when the view's
/// set is the Dio's whole set: [`drop_vanished`](DioInner::drop_vanished)
/// leaves rows with a flash in flight or settled since `mark`. Each dropped
/// row is announced as a removal and published to the other Dios over the
/// table. A failure is logged; the next whole-set re-pull tries again.
pub(super) async fn drop_unlisted(
    state: &SharedRows,
    dio: &DioInner,
    writer: &ChunkSink,
    mark: &SettleMark,
) {
    if state.view_narrows() {
        return;
    }
    let listed = writer.pushed_ids();
    let dropped = match dio.cache.list_values().await {
        Ok(cached) => dio.drop_vanished(cached.keys(), &listed, mark).await,
        Err(e) => Err(e),
    };
    let dropped = match dropped {
        Ok(dropped) => dropped,
        Err(e) => {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %dio.cache_table_name,
                error = %e,
                "dropping rows the master no longer lists failed",
            );
            return;
        }
    };
    if dropped.is_empty() {
        return;
    }
    tracing::debug!(
        target: "vantage_diorama::cache",
        table = %dio.master.name(),
        dropped = dropped.len(),
        "a whole-set re-pull dropped rows the master no longer lists",
    );
    for id in &dropped {
        let _ = dio
            .event_bus
            .send(DioEvent::RecordRemoved { id: id.clone() });
    }
    dio.publish_diff(Vec::new(), dropped);
}
