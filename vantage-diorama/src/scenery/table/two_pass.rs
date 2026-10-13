//! Two-pass loading: a sequential **list pass** that builds the per-query
//! index from cheap rows, and a viewport-driven **detail pass** that hydrates
//! expensive columns one id at a time.
//!
//! Engaged only for a Dio that owns augmentation (see
//! [`TableSceneryBuilder::open`](super::builder::TableSceneryBuilder::open)).
//! In single-pass mode none of this runs.
//!
//! - `list` — the list pass and the index refresh;
//! - `refine` — publishing the visible map, refined over the cache or not;
//! - `detail` — the detail pass and its failures.

mod detail;
mod list;
mod refine;

use vantage_types::Record;

use crate::dio::DioInner;
use crate::dio::query_index::index_key;
use crate::lens::CacheStatus;
use crate::scenery::enriched_record::EnrichedRecord;

use super::state::TableSceneryState;

pub(crate) use detail::{mark_detail_failed, resume_hydration, run_detail_for_range};
pub(crate) use list::{refresh_index, run_list_page, seed_from_index};
pub(crate) use refine::{reseed_filtered, resort, update_row_from_cache};

/// A cached row as the view shows it: `Fresh` once hydrated, `Incomplete`
/// before.
fn enriched(record: Record<ciborium::Value>, status: CacheStatus) -> EnrichedRecord {
    match status {
        CacheStatus::Complete => EnrichedRecord::fresh(record),
        CacheStatus::Incomplete => EnrichedRecord::incomplete(record),
    }
}

/// The shared-index key of the view's current `(conditions, op conditions,
/// sort)` variant.
fn variant_key(state: &TableSceneryState, dio_inner: &DioInner) -> String {
    let conditions = state.conditions.read().unwrap().clone();
    let op_conditions = state.op_conditions.read().unwrap().clone();
    let sort = state.sort.read().unwrap().clone();
    let mut key = index_key(&dio_inner.master, &conditions, sort.as_ref());
    key.push('\u{1}');
    key.push_str(&super::helpers::op_conditions_key(&op_conditions));
    key
}

/// Read `id`'s cached row and status through the view's reader.
async fn cached(
    state: &TableSceneryState,
    dio_inner: &DioInner,
    id: &str,
) -> Option<(Record<ciborium::Value>, CacheStatus)> {
    state
        .reader(dio_inner)
        .get_value_with_status(id)
        .await
        .ok()
        .flatten()
}
