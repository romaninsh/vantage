//! A two-pass view whose query or Dio residual names an augmented column
//! hydrates every listed row before it reads as settled, over a listing
//! larger than its viewport. Rows still waiting for their detail are never
//! judged on the gap: `Eq` doesn't drop them for good, and `Ne` doesn't
//! admit them.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Cache, Lens, LoadState, TableScenery, Tuning};
use vantage_vista::{Condition, FilterOp, Vista};

use super::big_bucket::{ROWS, augmentation, john_ids, listed_dio, master_shell, without_window};
use super::fixtures::ids;
use crate::support::spy_cache::{SpyCache, SpyTable};
use crate::support::{augmented_dio, eventually, wait_listed};

pub(super) fn name_of(scenery: &Arc<dyn TableScenery>, i: usize) -> Option<CborValue> {
    scenery
        .row(i)
        .and_then(|row| row.record.get("name").cloned())
}

/// Watch `scenery` until it reads `Complete`; see [`settle_as`].
pub(super) async fn settle(
    scenery: &Arc<dyn TableScenery>,
    shown: impl Fn(&Arc<dyn TableScenery>),
) -> Vec<String> {
    settle_as(scenery, LoadState::Complete, shown).await
}

/// Watch `scenery` until it reads `want`, checking `shown` against every
/// state it passes through on the way, each of which must be `Loading`, and
/// return the ids it settled on, sorted.
pub(super) async fn settle_as(
    scenery: &Arc<dyn TableScenery>,
    want: LoadState,
    shown: impl Fn(&Arc<dyn TableScenery>),
) -> Vec<String> {
    let mut generations = scenery.subscribe();
    let complete = async {
        loop {
            shown(scenery);
            let state = scenery.load_state();
            if state == want {
                return;
            }
            assert_eq!(state, LoadState::Loading, "read before {want:?}");
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    tokio::time::timeout(Duration::from_secs(10), complete)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "never settled: {:?}, {} rows shown",
                scenery.load_state(),
                scenery.row_count()
            )
        });
    let mut settled = ids(scenery);
    settled.sort();
    settled
}

/// Every row shown is a John that has hydrated.
fn only_johns(scenery: &Arc<dyn TableScenery>) {
    for i in 0..scenery.row_count() {
        assert_eq!(name_of(scenery, i), Some(CborValue::Text("John".into())));
    }
}

#[tokio::test]
async fn a_view_over_a_dio_narrowed_on_an_augmented_eq_shows_every_john() -> Result<()> {
    let dio = listed_dio(Vista::new("bucket", master_shell())).await;
    let johns = dio
        .derive()
        .with_condition(Condition::new("name", FilterOp::Eq, "John"))
        .build()
        .await?;
    assert_eq!(johns.residual().conditions.len(), 1, "augmented: residual");
    let view = johns.table_scenery().page_size(20).open().await?;
    view.set_viewport(0..20, 0..0);
    assert_eq!(settle(&view, only_johns).await, john_ids());
    Ok(())
}

#[tokio::test]
async fn a_view_filtered_on_an_augmented_column_shows_every_john() -> Result<()> {
    let dio = listed_dio(Vista::new("bucket", master_shell())).await;
    let view = dio
        .table_scenery()
        .page_size(20)
        .filter(Condition::new("name", FilterOp::Eq, "John"))
        .open()
        .await?;
    view.set_viewport(0..20, 0..0);
    assert_eq!(settle(&view, only_johns).await, john_ids());
    Ok(())
}

#[tokio::test]
async fn an_augmented_ne_never_admits_an_unhydrated_row() -> Result<()> {
    let dio = listed_dio(Vista::new("bucket", master_shell())).await;
    let others = dio
        .derive()
        .with_condition(Condition::new("name", FilterOp::Ne, "John"))
        .build()
        .await?;
    let view = others.table_scenery().page_size(20).open().await?;
    view.set_viewport(0..20, 0..0);
    let settled = settle(&view, |scenery| {
        for i in 0..scenery.row_count() {
            assert_eq!(
                name_of(scenery, i),
                Some(CborValue::Text("Jane".into())),
                "row {i} shown before it hydrated, or a John"
            );
        }
    })
    .await;
    assert_eq!(settled.len(), 180, "every Jane, and only them");
    Ok(())
}

/// Ruling N2 lists a refined view's index to the end. Over a master without
/// `Window`, each page would be a full listing cut down; the view lists the
/// master once instead.
#[tokio::test]
async fn a_refined_view_over_a_master_without_window_lists_it_once() -> Result<()> {
    let (master, lists) = without_window();
    let dio = listed_dio(master).await;
    let view = dio
        .table_scenery()
        .page_size(10)
        .filter(Condition::new("kind", FilterOp::Eq, "red"))
        .open()
        .await?;
    wait_listed(&view).await;
    assert_eq!(view.row_count(), 100, "every red object");
    assert_eq!(lists.load(Ordering::SeqCst), 1, "one listing for 20 pages");
    Ok(())
}

/// Every row shown has `kind` = `want`, and there are 100 of them.
fn all_of_kind(scenery: &Arc<dyn TableScenery>, want: &str) -> bool {
    scenery.row_count() == 100
        && (0..100).all(|i| {
            scenery
                .row(i)
                .is_some_and(|row| row.record.get("kind") == Some(&CborValue::Text(want.into())))
        })
}

/// Runtime filters never key the shared two-pass index: switching from one
/// filter set to another refines the index already listed, with no listing.
#[tokio::test]
async fn switching_runtime_filters_lists_the_master_no_more() -> Result<()> {
    let (master, lists) = without_window();
    let dio = listed_dio(master).await;
    let view = dio.table_scenery().page_size(10).open().await?;
    wait_listed(&view).await;

    view.set_filters(vec![Condition::new("kind", FilterOp::Eq, "red")]);
    assert!(
        eventually(Duration::from_secs(10), || all_of_kind(&view, "red")).await,
        "the red filter never settled; {} rows",
        view.row_count()
    );
    let listed = lists.load(Ordering::SeqCst);

    view.set_filters(vec![Condition::new("kind", FilterOp::Eq, "blue")]);
    assert!(
        eventually(Duration::from_secs(10), || all_of_kind(&view, "blue")).await,
        "the blue filter never settled; {} rows",
        view.row_count()
    );
    assert_eq!(
        lists.load(Ordering::SeqCst),
        listed,
        "a new filter set listed the master again"
    );
    Ok(())
}

/// A view refining on an augmented column re-derives its rows once per batch
/// of hydrations, not once per hydrated row. Every re-derivation reads each
/// listed row with its status, so the cache's status reads over the row
/// count bound how many there were.
#[tokio::test]
async fn a_sweep_reseeds_once_per_batch_not_once_per_row() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(20)))
        .build()?;
    let master = Vista::new("bucket", master_shell());
    let dio = augmented_dio(&lens, master, vec![augmentation()]).await;
    let view = dio
        .table_scenery()
        .page_size(20)
        .filter(Condition::new("size", FilterOp::Gt, 0))
        .open()
        .await?;
    view.set_viewport(0..20, 0..0);
    assert_eq!(
        settle(&view, |_| {}).await.len(),
        ROWS - 1,
        "every size but 0"
    );
    // The view settles on the first reseed after the last hydration; the
    // ones still queued behind it run after.
    let reads = || table.status_reads.load(Ordering::SeqCst);
    let mut seen = reads();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if reads() == seen {
            break;
        }
        seen = reads();
    }
    let passes = seen / ROWS;
    assert!(passes <= 12, "{passes} passes over the index for one sweep");
    Ok(())
}
