//! Every open returns a view its caller owns. Views of one query share the
//! fetched rows underneath: one fetch and one count, however many views.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;
use vantage_core::Result;
use vantage_diorama::{LoadState, Loader};
use vantage_vista::{Condition, FilterOp, SortDirection, Vista};

use support::views::{ids, paged, rest_loader, serve, shell};
use support::{eager_dio, eventually};

/// Holds every load until released; re-armed, it holds the next ones.
#[derive(Default)]
struct Gate {
    open: AtomicBool,
    notify: Notify,
}

impl Gate {
    async fn pass(&self) {
        loop {
            // Created before the check, so a release in between still wakes it.
            let released = self.notify.notified();
            if self.open.load(Ordering::SeqCst) {
                return;
            }
            released.await;
        }
    }

    fn release(&self) {
        self.open.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn arm(&self) {
        self.open.store(false, Ordering::SeqCst);
    }
}

/// [`rest_loader`], but every load waits at `gate` before it reads anything.
fn gated_loader(gate: Arc<Gate>) -> Loader {
    Loader::paged(move |cx, range, query, sink| {
        let gate = gate.clone();
        async move {
            gate.pass().await;
            serve(cx, range, query, sink).await
        }
    })
}

#[tokio::test]
async fn two_views_of_one_query_issue_one_fetch_and_one_count() -> Result<()> {
    let shell = shell();
    let probe = shell.clone();
    let dio = paged(shell, rest_loader()).await?;
    let first = dio.table_scenery().page_size(10).open().await?;
    let second = dio.table_scenery().page_size(10).open().await?;
    first.set_viewport(0..10, 0..0);
    second.set_viewport(0..10, 0..0);
    let both = || ids(&first).len() == 3 && ids(&second).len() == 3;
    assert!(eventually(Duration::from_secs(5), both).await);
    tokio::time::sleep(Duration::from_millis(200)).await; // past the viewport debounce
    let calls = probe.calls();
    assert_eq!(
        (calls.fetch_window, calls.count),
        (1, 1),
        "two views of one query: {calls:?}"
    );

    // A view of another query fetches for itself, and the first two are untouched.
    let third = dio.table_scenery().page_size(10).open().await?;
    third.set_search(Some("beta".into()));
    assert!(eventually(Duration::from_secs(5), || ids(&third) == ["b"]).await);
    let calls = probe.calls();
    assert_eq!(
        (calls.fetch_window, calls.count),
        (2, 2),
        "a third query: {calls:?}"
    );
    assert_eq!(ids(&first), ["a", "b", "c"]);
    Ok(())
}

#[tokio::test]
async fn a_set_on_one_view_never_reshapes_another() -> Result<()> {
    let dio = eager_dio(Vista::new("t", shell())).await;
    let first = dio.table_scenery().open().await?;
    let second = dio.table_scenery().open().await?;
    first.set_order(Some(("name".into(), SortDirection::Descending)));
    first.set_filters(vec![Condition::new("name", FilterOp::Ne, "beta")]);
    assert!(
        eventually(Duration::from_secs(5), || ids(&first) == ["c", "a"]).await,
        "the first view never narrowed; shows {:?}",
        ids(&first)
    );
    assert_eq!(ids(&second), ["a", "b", "c"]);
    assert_eq!(second.view_stats().order, None);
    Ok(())
}

#[tokio::test]
async fn a_repointed_view_joins_a_live_view_of_its_new_query() -> Result<()> {
    let shell = shell();
    let probe = shell.clone();
    let dio = paged(shell, rest_loader()).await?;
    let plain = dio.table_scenery().page_size(10).open().await?;
    let searched = dio.table_scenery().page_size(10).open().await?;
    let both = || ids(&plain).len() == 3 && ids(&searched).len() == 3;
    assert!(eventually(Duration::from_secs(5), both).await);

    searched.set_search(Some("beta".into()));
    assert!(eventually(Duration::from_secs(5), || ids(&searched) == ["b"]).await);

    plain.set_search(Some("beta".into()));
    assert!(eventually(Duration::from_secs(5), || ids(&plain) == ["b"]).await);
    tokio::time::sleep(Duration::from_millis(200)).await; // past the viewport debounce
    let calls = probe.calls();
    assert_eq!(
        (calls.fetch_window, calls.count),
        (2, 2),
        "the re-pointed view joined the live beta rows: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_load_in_flight_for_the_old_query_never_lands_in_the_repointed_view() -> Result<()> {
    let gate = Arc::new(Gate::default());
    let dio = paged(shell(), gated_loader(gate.clone())).await?;
    let view = dio.table_scenery().page_size(10).open().await?;
    tokio::time::sleep(Duration::from_millis(100)).await; // the on-open load parks on the gate
    view.set_search(Some("gamma".into()));
    gate.release();

    for _ in 0..60 {
        let shown = ids(&view);
        assert!(
            !shown.iter().any(|id| id == "a" || id == "b"),
            "the old query's rows reached the re-pointed view: {shown:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        eventually(Duration::from_secs(5), || ids(&view) == ["c"]).await,
        "the view never showed its own query's rows; shows {:?}",
        ids(&view)
    );
    Ok(())
}

#[tokio::test]
async fn reopening_a_query_after_its_last_view_closed_loads_again() -> Result<()> {
    let shell = shell();
    let probe = shell.clone();
    let dio = paged(shell, rest_loader()).await?;
    let view = dio.table_scenery().page_size(10).open().await?;
    assert!(eventually(Duration::from_secs(5), || ids(&view).len() == 3).await);
    tokio::time::sleep(Duration::from_millis(200)).await; // past the viewport debounce
    drop(view);

    let again = dio.table_scenery().page_size(10).open().await?;
    assert!(eventually(Duration::from_secs(5), || ids(&again).len() == 3).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        probe.calls().fetch_window,
        2,
        "the reopened query loaded its own rows"
    );
    #[cfg(feature = "diagnostics")]
    assert_eq!(dio.diagnostics().await?.sceneries.len(), 1);
    Ok(())
}

#[tokio::test]
async fn copy_on_write_keeps_the_rows_on_screen_while_the_new_query_loads() -> Result<()> {
    let gate = Arc::new(Gate::default());
    let dio = paged(shell(), gated_loader(gate.clone())).await?;
    let view = dio.table_scenery().page_size(10).open().await?;
    gate.release();
    assert!(eventually(Duration::from_secs(5), || ids(&view).len() == 3).await);
    tokio::time::sleep(Duration::from_millis(200)).await; // past the viewport debounce

    gate.arm();
    view.set_search(Some("alpha".into()));
    assert_eq!(view.row_count(), 3, "the copied rows stay on screen");
    assert_ne!(view.load_state(), LoadState::Loading);

    gate.release();
    assert!(
        eventually(Duration::from_secs(5), || ids(&view) == ["a"]).await,
        "the new query's rows never replaced the copy; shows {:?}",
        ids(&view)
    );
    Ok(())
}

#[tokio::test]
async fn closing_the_last_view_mid_load_stops_the_fetch() -> Result<()> {
    let shell = shell();
    let probe = shell.clone();
    let gate = Arc::new(Gate::default());
    let dio = paged(shell, gated_loader(gate.clone())).await?;
    let view = dio.table_scenery().page_size(10).open().await?;
    view.set_viewport(0..10, 0..0);
    tokio::time::sleep(Duration::from_millis(150)).await; // past the debounce: parked on the gate
    drop(view);

    gate.release();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let calls = probe.calls();
    assert_eq!(
        (calls.fetch_window, calls.count),
        (0, 0),
        "a closed grid kept pulling: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn closing_the_last_view_inside_the_debounce_stops_the_fetch() -> Result<()> {
    let shell = shell();
    let probe = shell.clone();
    let dio = paged(shell, rest_loader()).await?;
    let view = dio.table_scenery().page_size(10).open().await?;
    view.set_viewport(0..10, 0..0);
    // Let the loop take the request and hold the rows inside the debounce.
    tokio::time::sleep(Duration::from_millis(10)).await;
    drop(view); // well inside the 50 ms debounce

    tokio::time::sleep(Duration::from_millis(300)).await;
    let calls = probe.calls();
    assert_eq!(
        (calls.fetch_window, calls.count),
        (0, 0),
        "a closed grid kept pulling: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_builder_search_is_the_initial_search() -> Result<()> {
    let dio = eager_dio(Vista::new("t", shell())).await;
    let view = dio.table_scenery().search("beta").open().await?;
    assert!(
        eventually(Duration::from_secs(5), || ids(&view) == ["b"]).await,
        "shows {:?}",
        ids(&view)
    );
    assert_eq!(view.view_stats().search.as_deref(), Some("beta"));
    Ok(())
}
