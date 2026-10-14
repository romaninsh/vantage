//! Counting the rows a view would hold under a replacement set of runtime filters.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;
use vantage_core::{Context as _, Result, error};
use vantage_vista::{Condition, VistaQuery};

use crate::dio::{AbortOnDrop, DioInner};

use super::plan::{Serving, admitted_by_view};
use super::state::SharedRows;
use super::view::TableView;

/// See [`TableScenery::count_where`](super::TableScenery::count_where).
///
/// The work runs on the lens's runtime and the returned future only awaits
/// its join handle, so the caller's executor needs no tokio context. Dropping
/// the future aborts the work.
///
/// A two-pass view gets no count: its rows are the per-query index, which the
/// cache does not mirror (the cache also holds rows other queries listed), and
/// the index is still growing while list pages arrive.
pub(super) fn count_where(
    view: &TableView,
    filters: Vec<Condition>,
) -> BoxFuture<'static, Result<Option<usize>>> {
    let shared = view.shared();
    if shared.two_pass {
        return Box::pin(async { Ok(None) });
    }
    let Some(dio) = view.dio_weak.upgrade() else {
        return Box::pin(async { Ok(None) });
    };
    let mut query = view.query_with_filters(&filters);
    query.order = None;
    let runtime = dio.lens.runtime.clone();
    let task = AbortOnDrop(runtime.spawn(count(shared, dio, query)));
    Box::pin(async move { task.await.context(error!("count_where task failed"))? })
}

/// See [`TableScenery::filters_supported`](super::TableScenery::filters_supported):
/// the view's query with `filters` in place of its own, planned against the
/// Dio and decided as its rows decide their serving.
pub(super) fn filters_supported(view: &TableView, filters: &[Condition]) -> bool {
    answerable(view, view.query_with_filters(filters))
}

/// See [`TableScenery::search_supported`](super::TableScenery::search_supported):
/// the view's query with a probe search in place of its own, decided as
/// [`filters_supported`] decides. A paged view whose master can't search
/// answers yes when its Lens copies the whole set to search over.
pub(super) fn search_supported(view: &TableView) -> bool {
    let mut query = view.view_query();
    query.search = Some("x".into());
    answerable(view, query)
}

/// Whether `query`, planned as the view's rows plan their own, is served
/// rather than failed.
fn answerable(view: &TableView, query: VistaQuery) -> bool {
    let Some(dio) = view.dio_weak.upgrade() else {
        return false;
    };
    let shared = view.shared();
    let (offered, sugar, planned) = shared.plan_query(&dio, query);
    let serving = Serving::decide(&shared, &dio, &offered, &sugar, planned.as_ref());
    !matches!(serving, Serving::Failed(_))
}

/// `query` (the view query with the replacement filters, unordered), planned
/// as the view's rows plan their own. A query the master rejects has no
/// count. A paged view whose master takes all of it counts at the master;
/// one that leaves a clause over counts the whole copy its Lens's eager start
/// makes, and has no count without one. An eager view counts its cache once
/// it has settled.
async fn count(
    state: Arc<SharedRows>,
    dio: Arc<DioInner>,
    query: VistaQuery,
) -> Result<Option<usize>> {
    let (offered, sugar, planned) = state.plan_query(&dio, query.clone());
    let plan = match planned {
        Ok(plan) => plan,
        Err(e) => {
            tracing::debug!(
                target: "vantage_diorama::filter",
                table = %dio.master.name(),
                error = %e,
                "count_where: the master rejected the query — no count",
            );
            return Ok(None);
        }
    };
    if state.paged {
        if !plan.has_residual() && sugar.is_empty() {
            let n = dio.plan(&offered)?.narrowed.count().await?;
            return Ok(usize::try_from(n).ok());
        }
        if dio.lens.callbacks.loader.eager.is_none() {
            return Ok(None);
        }
        dio.copy_whole().await?;
    } else if !state.settled.load(Ordering::SeqCst) {
        return Ok(None);
    }
    count_in_cache(&state, &dio, &query).await.map(Some)
}

/// The cached rows `query` and the Dio's residual admit, read as the view
/// reads them. Rows still missing augmented values a clause names are
/// hydrated first, so no gap is counted or dropped.
async fn count_in_cache(
    state: &SharedRows,
    dio: &Arc<DioInner>,
    query: &VistaQuery,
) -> Result<usize> {
    let reader = state.reader(dio);
    let mut rows = reader.list_values().await?;
    if dio.names_augmented(query) {
        dio.hydrate(&mut rows).await?;
        if state.read_cache.is_some() {
            // The hydrated rows are in the cache; the sugared reader adds
            // its outputs over them.
            rows = reader.list_values().await?;
        }
    }
    Ok(rows
        .values()
        .filter(|row| admitted_by_view(dio, query, row))
        .count())
}
