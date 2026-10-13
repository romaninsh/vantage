//! A paged view whose serving changes while the whole copy it waits on is
//! still running: the copy, landing late, never writes rows the new serving
//! doesn't own, and a view waiting on it reads as loading.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Clause, Dio, LoadState, OpCondition, TableScenery};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp};

use super::fixtures::{assert_fails, ids, items, open, wait_for_ids};
use crate::support::{memory_lens, page_master};

/// A Dio over [`items`] that can't search. Its views page windows; one that
/// searches is served from a whole copy, which waits for a permit of `gate`
/// and also carries a row `z` the master doesn't have, so a copy reaching
/// the visible map shows.
async fn gated_dio(gate: Arc<Semaphore>) -> Dio {
    let loader = page_master().or_eager(move |cx| {
        let gate = gate.clone();
        async move {
            gate.acquire()
                .await
                .map_err(|_| vantage_core::error!("gate closed"))?
                .forget();
            let mut rows = cx.master().list_values().await?;
            let mut z = Record::new();
            z.insert("id".into(), CborValue::Text("z".into()));
            z.insert("team".into(), CborValue::Text("red".into()));
            rows.insert("z".into(), z);
            cx.cache().insert_values(rows).await
        }
    });
    memory_lens(loader)
        .dio(items(&[Capability::Order, Capability::FilterOps]))
        .build()
        .await
        .expect("dio builds")
}

/// Open a view and let its first window land.
async fn windowed(dio: &Dio) -> Arc<dyn TableScenery> {
    let view = open(dio, |b| b).await;
    wait_for_ids(&view, &["a", "b", "c"]).await;
    view
}

/// Start the whole copy and give the reactor time to block on the gate.
async fn copying(view: &Arc<dyn TableScenery>) {
    view.set_search(Some("re".into()));
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn a_copy_landing_after_a_return_to_windows_leaves_the_window_rows() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let dio = gated_dio(gate.clone()).await;
    let view = windowed(&dio).await;
    copying(&view).await;

    view.set_search(None);
    wait_for_ids(&view, &["a", "b", "c"]).await;
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        ids(&view),
        ["a", "b", "c"],
        "the late copy installed nothing"
    );
    assert_eq!(view.load_state(), LoadState::Complete);
    Ok(())
}

#[tokio::test]
async fn a_copy_landing_after_a_failure_installs_no_rows() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let dio = gated_dio(gate.clone()).await;
    let view = windowed(&dio).await;
    copying(&view).await;

    view.set_filter_terms(vec![OpCondition::new("colour", FilterOp::Ne, "x")]);
    assert_fails(&view, Clause::Filter("colour".into()));
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_fails(&view, Clause::Filter("colour".into()));
    Ok(())
}

/// From a failure to the whole copy, the view reads as loading until the
/// copy lands, never as a complete empty set.
#[tokio::test]
async fn a_failed_view_waiting_on_the_copy_reads_as_loading() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let dio = gated_dio(gate.clone()).await;
    let view = windowed(&dio).await;

    view.set_filters(vec![("colour".to_string(), "x".into())]);
    assert_fails(&view, Clause::Filter("colour".into()));
    view.set_search(Some("re".into()));
    assert_fails(&view, Clause::Filter("colour".into()));
    view.set_filters(Vec::new());
    assert_eq!(view.load_state(), LoadState::Loading);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(view.load_state(), LoadState::Loading, "still copying");

    gate.add_permits(1);
    wait_for_ids(&view, &["a", "c", "z"]).await;
    assert_eq!(view.load_state(), LoadState::Complete);
    Ok(())
}
