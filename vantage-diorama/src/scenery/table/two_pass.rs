//! Two-pass loading: a sequential **list pass** that builds the per-query
//! index from cheap rows, and a viewport-driven **detail pass** that hydrates
//! expensive columns one id at a time.
//!
//! Engaged only for a Dio that owns augmentation (see
//! [`TableSceneryBuilder::open`](super::builder::TableSceneryBuilder::open)).
//! In single-pass mode none of this runs.
//!
//! - `list` — the list pass;
//! - `reindex` — re-listing an index after a whole-set change;
//! - `refine` — publishing the visible map, refined over the cache or not;
//! - `coalesce` — one refined reseed per batch of moved rows;
//! - `detail` — the detail pass and its failures;
//! - `splice` — a confirmed insert or delete, applied to the index in place.

mod coalesce;
mod detail;
mod list;
mod refine;
mod reindex;
mod splice;

use vantage_types::Record;
use vantage_vista::VistaQuery;

use crate::dio::DioInner;
use crate::dio::query_index::index_key;
use crate::lens::CacheStatus;
use crate::scenery::enriched_record::EnrichedRecord;

use super::state::SharedRows;

pub(crate) use coalesce::{Coalescer, reseed_loop};
pub(crate) use detail::{mark_detail_failed, resume_hydration, run_detail_for_range};
pub(crate) use list::{run_list_page, seed_from_index};
pub(crate) use refine::{reseed_filtered, resort, update_row_from_cache};
pub(crate) use reindex::refresh_index;
pub(crate) use splice::{splice_inserted, splice_removed};

/// A cached row as the view shows it: `Fresh` once hydrated, `Incomplete`
/// before.
fn enriched(record: Record<ciborium::Value>, status: CacheStatus) -> EnrichedRecord {
    match status {
        CacheStatus::Complete => EnrichedRecord::fresh(record),
        CacheStatus::Incomplete => EnrichedRecord::incomplete(record),
    }
}

/// The shared-index key of the rows' `(base conditions, order)` variant.
/// Runtime filters and search never key the index: the list pass lists the
/// master without them, and `local_refine` applies them over the index.
pub(crate) fn variant_key(state: &SharedRows, dio_inner: &DioInner) -> String {
    let mut query = VistaQuery::new();
    query.conditions = state.base.clone();
    query.order = state.query.order.clone();
    index_key(&dio_inner.master, &query)
}

/// Read `id`'s cached row and status through the view's reader.
async fn cached(
    state: &SharedRows,
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
