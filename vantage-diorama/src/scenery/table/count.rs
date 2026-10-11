//! Counting the rows a view would hold under a replacement set of UI filter terms.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use tokio::task::JoinHandle;
use vantage_core::{Context as _, Result, error};
use vantage_vista::Vista;

use crate::dio::DioInner;

use super::helpers::{matches_conditions, matches_op_conditions, matches_search};
use super::{OpCondition, TableSceneryState};

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

async fn count(
    state: Arc<TableSceneryState>,
    dio: Arc<DioInner>,
    terms: Vec<OpCondition>,
) -> Result<Option<usize>> {
    if state.paged {
        count_at_master(&state, &dio, terms).await
    } else {
        count_in_cache(&state, &dio, &terms).await
    }
}

/// An eager view counts the rows its cache holds, through the reseed filter
/// chain with `terms` standing in for the UI terms. Before the view has
/// settled the cache may still be filling, so there is no count yet.
async fn count_in_cache(
    state: &TableSceneryState,
    dio: &DioInner,
    terms: &[OpCondition],
) -> Result<Option<usize>> {
    if !state.settled.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let all = state.reader(dio).list_values().await?;
    let conditions = state.conditions.read().unwrap().clone();
    let op_conditions = state.op_conditions.read().unwrap().clone();
    let ui_filters = state.ui_filters.read().unwrap().clone();
    let search = state.search.read().unwrap().clone();
    let n = all
        .values()
        .filter(|rec| matches_conditions(rec, &conditions))
        .filter(|rec| matches_conditions(rec, &ui_filters))
        .filter(|rec| matches_op_conditions(rec, &op_conditions))
        .filter(|rec| matches_op_conditions(rec, terms))
        .filter(|rec| matches_search(rec, search.as_deref()))
        .count();
    Ok(Some(n))
}

/// A paged view counts what its chunk fetches would read: a narrowed clone of
/// the master carrying the current search and `terms`. Any term the master
/// cannot take means no count — one that silently ignored a term would be
/// wider than the view it claims to describe.
async fn count_at_master(
    state: &TableSceneryState,
    dio: &DioInner,
    terms: Vec<OpCondition>,
) -> Result<Option<usize>> {
    let master = dio.master.read().unwrap().clone();
    let caps = master.capabilities();
    if !caps.can_count {
        return Ok(None);
    }
    let Some(shell) = master.source.clone_shell() else {
        return Ok(None);
    };
    let mut narrowed = Vista::new(master.name(), shell);

    let search = state.search.read().unwrap().clone();
    if let Some(text) = search.filter(|_| caps.can_search) {
        narrowed.add_search(text)?;
    }
    for term in terms {
        if !term.pushable_to(caps) {
            return Ok(None);
        }
        if let Err(e) = narrowed.add_condition(term.column.clone(), term.op, term.value) {
            tracing::debug!(
                target: "vantage_diorama::filter",
                table = %master.name(),
                column = %term.column,
                op = ?term.op,
                error = %e,
                "count_where: the master declined a filter term — no count",
            );
            return Ok(None);
        }
    }
    let n = narrowed.get_count().await?;
    Ok(usize::try_from(n).ok())
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
