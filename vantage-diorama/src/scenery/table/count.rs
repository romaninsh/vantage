//! Counting the rows a view would hold under a replacement set of UI filter terms.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use tokio::task::JoinHandle;
use vantage_core::{Context as _, Result, error};
use vantage_vista::VistaQuery;

use crate::dio::DioInner;

use super::OpCondition;
use super::plan::admitted_by_view;
use super::state::TableSceneryState;

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
    state: &Arc<TableSceneryState>,
    terms: Vec<OpCondition>,
) -> BoxFuture<'static, Result<Option<usize>>> {
    if state.two_pass {
        return Box::pin(async { Ok(None) });
    }
    let Some(dio) = state.dio_weak.upgrade() else {
        return Box::pin(async { Ok(None) });
    };
    let runtime = dio.lens.runtime.clone();
    let task = AbortOnDrop(runtime.spawn(count(state.clone(), dio, terms)));
    Box::pin(async move { task.await.context(error!("count_where task failed"))? })
}

/// A join handle that aborts its task when dropped, so a caller that stops
/// waiting for a count stops the query behind it.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Future for AbortOnDrop<T> {
    type Output = std::result::Result<T, tokio::task::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The view query with `terms` for its UI terms, planned as the view plans
/// its own. A query the master rejects has no count. A paged view whose
/// master takes all of it counts at the master; one that leaves a clause
/// over counts the whole copy its Lens's eager start makes, and has no
/// count without one. An eager view counts its cache once it has settled.
async fn count(
    state: Arc<TableSceneryState>,
    dio: Arc<DioInner>,
    terms: Vec<OpCondition>,
) -> Result<Option<usize>> {
    let mut query = state.query_with_terms(&terms);
    query.order = None;
    let (_, sugar, planned) = state.plan_query(&dio, query.clone());
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
            let n = plan.narrowed().count().await?;
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
    state: &TableSceneryState,
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::AbortOnDrop;

    #[tokio::test]
    async fn dropping_the_guard_aborts_the_task() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let guard = AbortOnDrop(tokio::spawn(async move {
            let _tx = tx;
            std::future::pending::<()>().await;
        }));
        drop(guard);
        // The never-finishing task only releases its sender when it is aborted.
        let closed = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .expect("task was not aborted");
        assert!(closed.is_err());
    }
}
