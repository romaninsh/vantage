//! A Lens hands out one live Dio per identity: two builds with the same cache
//! identity, the same augmentations and the same `live` setting share one
//! Dio, and so one eager start, one cache reader and one set of sceneries.

mod support;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use support::{MockView, copy_master, fixed_augmentation, memory_lens, meta, record};
use tokio::sync::mpsc;
use vantage_diorama::{Augmentation, Cache, CacheBackend, Lens, MemoryCache, Refresher};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Condition, FilterOp, JoinKey, Reference, Vista, VistaQuery};

/// Two red players and a blue one, with a `members` has-many onto `players`.
/// Returns the master, then handles on the team and player stores, whose
/// clones share their call counters.
fn teams() -> (Vista, MockShell, MockShell) {
    let players = MockShell::new()
        .with_metadata(meta(&["id", "team_id", "name"]))
        .with_record(
            "p1",
            record(&[("id", "p1"), ("team_id", "t1"), ("name", "Ann")]),
        )
        .with_record(
            "p2",
            record(&[("id", "p2"), ("team_id", "t2"), ("name", "Bob")]),
        );
    let shell = MockShell::new()
        .with_metadata(meta(&["id", "team"]).with_reference(Reference::has_many(
            "members",
            "players",
            JoinKey::new("id", "team_id"),
        )))
        .with_record("t1", record(&[("id", "t1"), ("team", "red")]))
        .with_record("t2", record(&[("id", "t2"), ("team", "blue")]))
        .with_record("t3", record(&[("id", "t3"), ("team", "red")]))
        .with_ref_target("members", players.clone());
    (Vista::new("teams", shell.clone()), shell, players)
}

/// Merges a `name` per team from a detail source keyed by team id.
fn team_names() -> Augmentation {
    let names = ["t1", "t2", "t3"].into_iter().fold(
        MockShell::new().with_metadata(meta(&["id", "name"])),
        |shell, id| shell.with_record(id, record(&[("id", id), ("name", "Team")])),
    );
    fixed_augmentation(Vista::new("team_names", names), &["name"])
}

fn ids(view: &MockView) -> Vec<Option<String>> {
    (0..view.row_count())
        .map(|i| view.col_at(i, "id"))
        .collect()
}

#[tokio::test]
async fn two_derivations_of_one_query_share_one_dio() {
    let (master, probe, _) = teams();
    let dio = memory_lens(copy_master())
        .dio(master)
        .build()
        .await
        .expect("dio builds");
    let q = VistaQuery::new().with_condition(Condition::new("team", FilterOp::Eq, "red"));

    let first = dio.derive().with_query(&q).build().await.expect("derives");
    let second = dio.derive().with_query(&q).build().await.expect("derives");
    assert_eq!(
        probe.calls().list,
        2,
        "the parent's eager start and one derived eager start"
    );

    let a = MockView::open(&first, 10).await;
    let b = MockView::open(&second, 10).await;
    a.settle_until("two red rows", |v| v.loaded_rows() == 2)
        .await;
    b.settle_until("two red rows", |v| v.loaded_rows() == 2)
        .await;
    assert_eq!(ids(&a), ids(&b));
}

#[tokio::test]
async fn two_get_refs_to_one_target_share_one_dio() {
    let (master, _, players) = teams();
    let dio = memory_lens(copy_master())
        .dio(master)
        .build()
        .await
        .expect("dio builds");
    let t1 = record(&[("id", "t1"), ("team", "red")]);

    let _first = dio.get_ref("members", &t1).await.expect("traverses");
    let _second = dio.get_ref("members", &t1).await.expect("traverses");
    assert_eq!(players.calls().list, 1, "one eager start for the target");
}

#[tokio::test]
async fn the_same_vista_with_other_augmentations_builds_its_own_dio() {
    let (master, probe, _) = teams();
    let lens = memory_lens(copy_master());

    let plain = lens.dio(master.clone()).build().await.expect("dio builds");
    let augmented = lens
        .dio(master)
        .augment(vec![team_names()])
        .build()
        .await
        .expect("dio builds");
    assert_eq!(probe.calls().list, 2, "each Dio ran its own eager start");

    // Checked before the augmented Dio hydrates: both read the one `teams`
    // cache table, so merged names would show here afterwards.
    let single = MockView::open(&plain, 10).await;
    single
        .settle_until("three rows", |v| v.loaded_rows() == 3)
        .await;
    assert!(
        (0..3).all(|i| single.col_at(i, "name").is_none()),
        "the plain Dio has no augmented columns"
    );

    let two_pass = MockView::open(&augmented, 10).await;
    two_pass.viewport(0..3);
    two_pass
        .settle_until("names merged", |v| v.col_at(0, "name").is_some())
        .await;
}

/// Two builds of one identity racing each other end on one Dio: a change
/// applied through one is announced on the other's bus.
#[tokio::test(flavor = "multi_thread")]
async fn racing_builds_of_one_identity_share_one_dio() {
    let (master, _, _) = teams();
    let lens = memory_lens(copy_master());
    let (first, second) = tokio::join!(lens.dio(master.clone()).build(), lens.dio(master).build());
    let (first, second) = (first.expect("builds"), second.expect("builds"));

    let mut bus = first.subscribe_events();
    second
        .apply_change(vantage_diorama::ChangeEvent::Inserted {
            id: "t9".into(),
            new: Some(record(&[("id", "t9"), ("team", "green")])),
        })
        .await
        .expect("applies");
    let announced = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(vantage_diorama::DioEvent::RecordInserted { id }) = bus.recv().await
                && id == "t9"
            {
                return;
            }
        }
    })
    .await;
    assert!(announced.is_ok(), "the other build's bus never saw it");
}

#[tokio::test]
async fn a_dropped_dio_is_rebuilt_not_revived() {
    let (master, probe, _) = teams();
    let lens = memory_lens(copy_master());

    let dio = lens.dio(master.clone()).build().await.expect("dio builds");
    drop(dio);
    let _again = lens.dio(master).build().await.expect("dio builds");
    assert_eq!(probe.calls().list, 2, "the rebuild ran its own eager start");
}

/// A shared Dio may be read only through its facade or a servo, which open
/// no scenery, so the build that shares it is where a stale cache is caught.
#[tokio::test(start_paused = true)]
async fn a_build_that_shares_a_dio_still_checks_max_age() -> vantage_core::Result<()> {
    const TTL: Duration = Duration::from_secs(60);
    let (tx, mut refreshes) = mpsc::unbounded_channel();
    let refresher = Refresher::new(move |_cx| {
        let tx = tx.clone();
        async move {
            let _ = tx.send(());
            Ok(())
        }
    });
    let backend = Arc::new(MemoryCache::new());
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .refresher(refresher.every(TTL * 10).max_age(TTL))
        .build()?;
    let table = backend.open_table("teams").await?;
    let (master, _, _) = teams();

    table.set_meta_refreshed_at(SystemTime::now()).await?;
    let _held = lens.dio(master.clone()).build().await?;
    let wait = Duration::from_secs(1);
    assert!(
        tokio::time::timeout(wait, refreshes.recv()).await.is_err(),
        "fresh at the first build"
    );

    table
        .set_meta_refreshed_at(SystemTime::now() - TTL * 2)
        .await?;
    let _shared = lens.dio(master).build().await?;
    assert!(
        tokio::time::timeout(wait, refreshes.recv()).await.is_ok(),
        "the sharing build refreshed the stale cache"
    );
    Ok(())
}
