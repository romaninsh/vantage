//! A view query the master rejects outright (an unknown column): a paged
//! view fails at once, without calling its loader and without showing the
//! rows of the query before.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vantage_core::Result;
use vantage_diorama::{Clause, Dio, LoadState, Loader};
use vantage_vista::{Capability, Condition, FilterOp};

use super::fixtures::{assert_fails, assert_fails_on, items, open, wait_for_ids};
use crate::support::memory_lens;

/// `colour = red`: [`items`] has no `colour` column.
fn red_colour() -> Condition {
    Condition::new("colour", FilterOp::Eq, "red")
}

/// A paged Dio over [`items`] whose loader counts its calls and reads each
/// window through `LoaderCx::fetch`.
async fn counted_paged_dio() -> (Dio, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let loader = Loader::paged(move |cx, range, query, sink| {
        counted.fetch_add(1, Ordering::SeqCst);
        async move {
            let fetched = cx.fetch(range.clone(), &query).await?;
            sink.set_total(fetched.page.rows.len());
            for (i, (id, row)) in fetched.page.rows.into_iter().enumerate() {
                sink.push(range.start + i, id, row).await?;
            }
            Ok(())
        }
    });
    let all = &[Capability::Order, Capability::Search, Capability::FilterOps];
    let dio = memory_lens(loader)
        .dio(items(all))
        .build()
        .await
        .expect("dio builds");
    (dio, calls)
}

/// A cold paged open with a rejected condition leaves `Loading` at once, as
/// a failure naming the column, and never calls the loader.
#[tokio::test]
async fn a_cold_paged_open_with_a_rejected_query_fails() -> Result<()> {
    let (dio, calls) = counted_paged_dio().await;
    let view = open(&dio, |b| b.filter(red_colour())).await;
    assert_fails(&view, Clause::Filter("colour".into()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "the loader is never called"
    );
    Ok(())
}

/// An eager view and a two-pass view fail on a rejected query as a paged one
/// does, rather than read as a complete empty set.
#[tokio::test]
async fn eager_and_two_pass_views_fail_on_a_rejected_query() -> Result<()> {
    let all = &[Capability::Order, Capability::Search, Capability::FilterOps];
    let eager = crate::support::eager_dio(items(all)).await;
    let view = open(&eager, |b| b.filter(red_colour())).await;
    assert_fails(&view, Clause::Filter("colour".into()));

    let two_pass = crate::support::bucket_dio().await;
    let view = open(&two_pass, |b| b.filter(red_colour())).await;
    assert_fails_on(&view, "bucket", Clause::Filter("colour".into()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_fails_on(&view, "bucket", Clause::Filter("colour".into()));
    Ok(())
}

/// A two-pass view and an eager one coming back from a rejected query read
/// as loading until their reload lands, never as a complete empty set.
#[tokio::test]
async fn eager_and_two_pass_views_leaving_a_failure_read_as_loading() -> Result<()> {
    let all = &[Capability::Order, Capability::Search, Capability::FilterOps];
    let eager = crate::support::eager_dio(items(all)).await;
    let two_pass = crate::support::bucket_dio().await;
    for (dio, want) in [(&eager, ["a", "b", "c"]), (&two_pass, ["o1", "o2", "o3"])] {
        let view = open(dio, |b| b).await;
        wait_for_ids(&view, &want).await;
        view.set_filters(vec![red_colour()]);
        assert!(matches!(view.load_state(), LoadState::Failed(_)));
        view.set_filters(Vec::new());
        assert_eq!(view.load_state(), LoadState::Loading, "{want:?}");
        wait_for_ids(&view, &want).await;
        assert_eq!(view.load_state(), LoadState::Complete, "{want:?}");
    }
    Ok(())
}

/// Filter chips naming a rejected column replace the rows on screen with a
/// failure at once, rather than leaving the previous query's answer up.
#[tokio::test]
async fn set_filters_to_a_rejected_column_withholds_the_rows() -> Result<()> {
    let (dio, calls) = counted_paged_dio().await;
    let view = open(&dio, |b| b).await;
    wait_for_ids(&view, &["a", "b", "c"]).await;

    let before = calls.load(Ordering::SeqCst);
    view.set_filters(vec![red_colour()]);
    assert_fails(&view, Clause::Filter("colour".into()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_fails(&view, Clause::Filter("colour".into()));
    assert_eq!(calls.load(Ordering::SeqCst), before, "no load for it");

    // Clearing the chips loads the view from the master again.
    view.set_filters(Vec::new());
    wait_for_ids(&view, &["a", "b", "c"]).await;
    Ok(())
}
