//! A detail source that legitimately has no record for a row must be asked
//! once, not forever.
//!
//! When a row hydrates and the detail source returns nothing, the Dio records
//! it in its settled-empty set so the detail sweep stops re-fetching it. That
//! set is what stops `gap → hydrate → still gapped → sweep again` from becoming
//! a permanent background loop.
//!
//! The regression these tests pin: the refresh pass used to clear the whole set
//! unconditionally, so the *scheduled* ticker wiped the answer every few
//! seconds and the sweep re-asked the same absent rows for the life of the
//! process. Harmless when every row has a detail record (the set is empty, so
//! clearing it costs nothing) — which is why same-source augmentation never
//! showed it. With a cross-source augment whose records are mostly absent, it
//! is a continuous re-fetch of every missing row, each one rewriting its row
//! and broadcasting `RecordChanged`, i.e. a permanently repainting UI.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use support::chunk::col_at;
use support::{meta, record};
use tempfile::TempDir;
use vantage_diorama::{
    Augmentation, Detail, Dio, Fetch, Lens, MergeRule, RowStatus, Source, TableScenery,
};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;
use vantage_vista_factory::VistaCatalog;

/// Master: two rows, cheap columns only.
fn master_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("branch", "main")]))
        .with_record("r1", record(&[("id", "r1"), ("branch", "dev")]))
        .with_metadata(meta(&["id", "branch"]));
    Vista::new("runs", Box::new(shell))
}

/// Detail source with a record for `r0` ONLY. `r1` is the row that settles
/// empty — the shape a cross-source augment hits constantly (an evidence row
/// with no matching bucket folder).
fn partial_detail_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("detail", "full-r0")]))
        .with_metadata(meta(&["id", "detail"]));
    Vista::new("runs-detail", Box::new(shell))
}

/// A catalog whose loader counts how many times the detail vista is built —
/// `Augmentation::base_vista` builds one per fetch, so this is an exact count
/// of detail fetches attempted.
fn counting_catalog(hits: Arc<AtomicUsize>) -> Arc<VistaCatalog> {
    let mut c = VistaCatalog::new();
    c.register(
        "runs-detail",
        Arc::new(move || {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(partial_detail_vista())
        }),
    );
    Arc::new(c)
}

fn aug() -> Augmentation {
    Augmentation {
        detail: Detail::Catalog("runs-detail".into()),
        source: Source::Id,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: vec!["detail".to_string()],
        },
    }
}

/// `refresh_every` only spawns the ticker when an `on_refresh` callback exists,
/// so register a no-op one. An augmenting Dio never calls it — it runs its own
/// reconciling pass — but its presence is what starts the timer, which is the
/// path under test.
async fn open_with_ticker(tmp: &TempDir, hits: Arc<AtomicUsize>, tick: Option<Duration>) -> Dio {
    let mut lens = Lens::new()
        .cache_at(tmp.path().join("cache.redb"))
        .viewport_debounce(Duration::from_millis(1));
    if let Some(tick) = tick {
        lens = lens
            .refresh_every(tick)
            .on_refresh(|_dio: &Dio| async move { Ok(()) });
    }
    let lens = Arc::new(lens.build().expect("lens builds"));
    let dio = lens.make_dio(master_vista()).await.expect("make_dio");
    dio.augment(counting_catalog(hits), vec![aug()])
}

fn status_of(s: &Arc<dyn TableScenery>, i: usize) -> Option<RowStatus> {
    s.row(i).map(|r| r.status.clone())
}

async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

/// Bring both rows through the detail pass once and return the fetch count.
async fn hydrate_both(dio: &Dio, hits: &AtomicUsize) -> (Arc<dyn TableScenery>, usize) {
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);
    eventually("r0 hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;
    // r1 has no detail record — it reaches Complete-in-cache carrying only its
    // list columns, so wait on the absence settling rather than on a status.
    eventually("detail pass quiesced", || {
        let before = hits.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));
        hits.load(Ordering::SeqCst) == before
    })
    .await;
    let settled = hits.load(Ordering::SeqCst);
    (scenery, settled)
}

/// The premise: a row whose detail source has no record still hydrates its
/// cheap columns, and the augmented column simply stays absent.
#[tokio::test]
async fn a_row_with_no_detail_record_keeps_its_list_columns() {
    let tmp = TempDir::new().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let dio = open_with_ticker(&tmp, hits.clone(), None).await;
    let (scenery, _) = hydrate_both(&dio, &hits).await;

    assert_eq!(col_at(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(
        col_at(&scenery, 1, "detail"),
        None,
        "r1's detail source has no record — the column stays absent",
    );
    assert_eq!(
        col_at(&scenery, 1, "branch").as_deref(),
        Some("dev"),
        "the cheap list columns survive a detail miss",
    );
}

/// THE REGRESSION. With the ticker running, a settled-empty row must not be
/// re-fetched on every tick.
///
/// Before the fix this failed loudly: the scheduled refresh cleared the
/// settled-empty set, so each tick re-fetched `r1` and the count climbed
/// without bound for as long as the process ran.
#[tokio::test]
async fn the_scheduled_ticker_does_not_re_probe_a_settled_empty_row() {
    let tmp = TempDir::new().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    // A 50ms tick: several refreshes land inside the observation window below.
    let dio = open_with_ticker(&tmp, hits.clone(), Some(Duration::from_millis(50))).await;
    let (_scenery, after_hydration) = hydrate_both(&dio, &hits).await;
    assert!(
        after_hydration >= 2,
        "both rows should have been fetched once: {after_hydration}",
    );

    // Let the ticker fire many times over.
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        after_hydration,
        "the scheduled refresh re-probed a row already known to have no detail \
         record — that is the sweep-never-converges loop",
    );
}

/// The other half of the contract: an EXPLICIT refresh is the user saying
/// "re-check everything", so it does re-probe. Without this the fix would just
/// be "never look again", and a detail record that appears later would be
/// unreachable.
#[tokio::test]
async fn an_explicit_refresh_re_probes_a_settled_empty_row() {
    let tmp = TempDir::new().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let dio = open_with_ticker(&tmp, hits.clone(), None).await;
    let (scenery, after_hydration) = hydrate_both(&dio, &hits).await;

    dio.refresh().await.expect("refresh");
    scenery.set_viewport(0..2);

    eventually("explicit refresh re-probed", || {
        hits.load(Ordering::SeqCst) > after_hydration
    })
    .await;
}
