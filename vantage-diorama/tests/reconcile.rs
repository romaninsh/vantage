//! Re-reading the whole set: `reconcile` leaves alone the rows a flash
//! settled while the snapshot was being listed and rewrites only the rows
//! that changed, a refresh re-runs the Lens's
//! eager start, and a refresher that reconciles is announced once.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tokio::sync::{Notify, Semaphore};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, ChangeFlash, Dio, DioEvent, Lens, Loader, Refresher};
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, forward_table_shell, mocks::MockShell};

use support::spy_cache::{SpyCache, SpyTable};
use support::{meta, record};

type Rows = IndexMap<String, Record<CborValue>>;

fn items() -> MockShell {
    MockShell::new()
        .with_metadata(meta(&["id", "name"]))
        .with_record("a", record(&[("id", "a"), ("name", "one")]))
        .with_record("b", record(&[("id", "b"), ("name", "two")]))
}

fn name_of(row: Option<Record<CborValue>>) -> Option<String> {
    match row?.get("name") {
        Some(CborValue::Text(name)) => Some(name.clone()),
        _ => None,
    }
}

/// Holds one listing, once armed: the rows are read, `listed` is signalled,
/// and the listing returns them only after a permit of `release`. The
/// snapshot it returns is therefore older than anything written meanwhile.
struct ListHold {
    armed: AtomicBool,
    listed: Notify,
    release: Semaphore,
}

impl Default for ListHold {
    fn default() -> Self {
        Self {
            armed: AtomicBool::new(false),
            listed: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

#[derive(Clone)]
struct HeldList {
    inner: Box<dyn TableShell>,
    hold: Arc<ListHold>,
}

forward_table_shell!(HeldList, inner, {
    async fn list(&self) -> Result<Rows> {
        let rows = self.inner.list().await?;
        if self.hold.armed.swap(false, Ordering::SeqCst) {
            self.hold.listed.notify_one();
            self.hold.release.acquire().await.expect("release").forget();
        }
        Ok(rows)
    }
});

fn held_master(hold: Arc<ListHold>) -> Vista {
    Vista::new("items", items()).map_shell(|inner| HeldList { inner, hold })
}

/// While a refresh holds its listing, a new row is inserted and an existing
/// one renamed, both settling before the listing returns. The refresh's
/// reconcile keeps both.
async fn assert_settled_flashes_survive(lens: Arc<Lens>) -> Result<()> {
    let hold = Arc::new(ListHold::default());
    let dio = lens.dio(held_master(hold.clone())).build().await?;
    dio.reconcile(dio.master().list_values().await?).await?;

    hold.armed.store(true, Ordering::SeqCst);
    let refresh = tokio::spawn({
        let dio = dio.clone();
        async move { dio.refresh().await }
    });
    hold.listed.notified().await;
    dio.flash(ChangeFlash::insert(
        "n",
        record(&[("id", "n"), ("name", "new")]),
    ))
    .await?;
    dio.flash(ChangeFlash::partial("a", record(&[("name", "edited")])))
        .await?;
    hold.release.add_permits(1);
    refresh.await.expect("refresh task")?;

    let facade = dio.cached();
    let new = facade.get_value("n").await?;
    assert_eq!(
        name_of(new).as_deref(),
        Some("new"),
        "a row inserted mid-list survives"
    );
    let edited = facade.get_value("a").await?;
    assert_eq!(
        name_of(edited).as_deref(),
        Some("edited"),
        "an edit settled mid-list survives"
    );
    assert!(facade.get_value("b").await?.is_some());
    Ok(())
}

/// The refresh with no refresher lists the master itself.
#[tokio::test]
async fn a_reread_keeps_flashes_settled_while_it_listed() -> Result<()> {
    assert_settled_flashes_survive(Lens::builder().cache(Cache::memory()).build()?).await
}

/// A refresher hands its listing to `LoaderCx::reconcile`, which knows when
/// the refresh began.
#[tokio::test]
async fn a_refresher_keeps_flashes_settled_while_it_listed() -> Result<()> {
    let refresher = Refresher::new(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    });
    let lens = Lens::builder()
        .cache(Cache::memory())
        .refresher(refresher)
        .build()?;
    assert_settled_flashes_survive(lens).await
}

/// An eager start that shapes what it copies: every row gains `shaped`.
fn shaping_start() -> Loader {
    Loader::eager(|cx| async move {
        let mut rows = cx.master().list_values().await?;
        for row in rows.values_mut() {
            row.insert("shaped".into(), CborValue::Bool(true));
        }
        cx.reconcile(rows).await
    })
}

/// A Dio the Lens copies whole re-reads through its eager start, not a raw
/// listing: after an `Invalidated` change the rows are still shaped, and a
/// row the master dropped is gone.
#[tokio::test]
async fn a_reread_runs_the_eager_start_again() -> Result<()> {
    let shell = items();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(shaping_start())
        .build()?;
    let dio = lens.dio(Vista::new("items", shell.clone())).build().await?;
    shell.remove_record("b");

    dio.apply_change(ChangeEvent::Invalidated).await?;
    let rows = dio.cached().list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
    assert_eq!(rows["a"].get("shaped"), Some(&CborValue::Bool(true)));
    Ok(())
}

/// A refresher that reconciles is announced once: one `DatasetChanged` and
/// one recorded refresh time per refresh.
#[tokio::test]
async fn a_reconciling_refresher_is_announced_once() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let refresher = Refresher::new(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    });
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .refresher(refresher)
        .build()?;
    let dio: Dio = lens.dio(Vista::new("items", items())).build().await?;
    let stamps_before = table.stamps.load(Ordering::SeqCst);
    let mut bus = dio.subscribe_events();

    dio.refresh().await?;
    let changed = std::iter::from_fn(|| bus.try_recv().ok())
        .filter(|e| matches!(e, DioEvent::DatasetChanged))
        .count();
    assert_eq!(changed, 1, "one DatasetChanged");
    assert_eq!(
        table.stamps.load(Ordering::SeqCst) - stamps_before,
        1,
        "one stamp"
    );
    Ok(())
}

/// A reconcile writes only the rows that changed: a snapshot equal to the
/// cache writes none, yet the refresh is still recorded and announced once.
#[tokio::test]
async fn an_unchanged_snapshot_writes_no_rows() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .build()?;
    let dio = lens.dio(Vista::new("items", items())).build().await?;
    let snapshot = dio.master().list_values().await?;
    dio.reconcile(snapshot.clone()).await?;
    assert_eq!(
        table.rows_written.load(Ordering::SeqCst),
        2,
        "the first copy"
    );

    let stamps_before = table.stamps.load(Ordering::SeqCst);
    let mut bus = dio.subscribe_events();
    let mut edited = snapshot.clone();
    edited["a"].insert("name".into(), CborValue::Text("edited".into()));
    dio.reconcile(snapshot).await?;
    assert_eq!(
        table.rows_written.load(Ordering::SeqCst),
        2,
        "nothing rewritten"
    );
    let changed = std::iter::from_fn(|| bus.try_recv().ok())
        .filter(|e| matches!(e, DioEvent::DatasetChanged))
        .count();
    assert_eq!(changed, 1, "one DatasetChanged");
    assert_eq!(
        table.stamps.load(Ordering::SeqCst) - stamps_before,
        1,
        "one stamp"
    );

    dio.reconcile(edited).await?;
    assert_eq!(
        table.rows_written.load(Ordering::SeqCst),
        3,
        "only the edited row"
    );
    Ok(())
}
