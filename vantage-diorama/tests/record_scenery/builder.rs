//! The record-scenery builder: `load_on_miss` and the `Incomplete` report.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Dio, Lens, RecordStatus};
use vantage_vista::mocks::MockShell;
use vantage_vista::{FilterOp, Vista, VistaCapabilities};

use crate::support::{
    augmented_dio, cond, eventually, fixed_augmentation, meta, quick_lens, record, size_detail,
    text,
};

const WITHIN: Duration = Duration::from_secs(10);

/// Rows `a` (red) and `b` (blue) with columns `id`, `name` and `team`.
fn team_shell() -> MockShell {
    MockShell::new()
        .with_metadata(meta(&["id", "name", "team"]))
        .with_record(
            "a",
            record(&[("id", "a"), ("name", "alpha"), ("team", "red")]),
        )
        .with_record(
            "b",
            record(&[("id", "b"), ("name", "beta"), ("team", "blue")]),
        )
}

/// A Dio over `shell` with an empty cache and no loader: nothing reaches the
/// cache unless a scenery puts it there.
async fn cold_dio(shell: &MockShell) -> Result<Dio> {
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    lens.dio(Vista::new("items", shell.clone())).build().await
}

#[tokio::test]
async fn load_on_miss_fetches_the_row_and_caches_it() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let shell = team_shell().gated(gate.clone());
    let dio = cold_dio(&shell).await?;

    let scenery = dio.record_scenery("a").load_on_miss().open().await?;
    assert_eq!(scenery.status(), RecordStatus::Loading);
    assert!(scenery.record().is_none());

    gate.add_permits(1);
    assert!(eventually(WITHIN, || scenery.status() == RecordStatus::Fresh).await);
    let row = scenery.record().expect("the fetched row");
    assert_eq!(row.record.get("name"), Some(&text("alpha")));
    assert_eq!(shell.calls().get, 1);

    let again = dio.record_scenery("a").open().await?;
    assert_eq!(again.status(), RecordStatus::Fresh, "the row was cached");
    assert_eq!(shell.calls().get, 1);
    Ok(())
}

#[tokio::test]
async fn load_on_miss_of_an_absent_id_is_not_found() -> Result<()> {
    let shell = team_shell();
    let dio = cold_dio(&shell).await?;

    let scenery = dio.record_scenery("zz").load_on_miss().open().await?;
    assert!(eventually(WITHIN, || scenery.status() == RecordStatus::NotFound).await);
    assert!(scenery.record().is_none());
    assert_eq!(shell.calls().get, 1);
    Ok(())
}

#[tokio::test]
async fn load_on_miss_never_caches_a_row_outside_the_dio() -> Result<()> {
    // Without `FilterOps` the master refuses `team != blue`, so the derived
    // Dio's residual holds it and the master's `get` still returns `b`.
    let shell = team_shell().with_capabilities(VistaCapabilities::new());
    let parent = cold_dio(&shell).await?;
    let dio = parent
        .derive()
        .with_query(&cond("team", FilterOp::Ne, "blue"))
        .build()
        .await?;
    assert_eq!(dio.residual().conditions.len(), 1, "the master refused it");

    let scenery = dio.record_scenery("b").load_on_miss().open().await?;
    assert!(eventually(WITHIN, || shell.calls().get == 1).await);
    assert!(eventually(WITHIN, || scenery.status() == RecordStatus::NotFound).await);
    assert!(scenery.record().is_none());
    assert_eq!(dio.cached().get_value("b").await?, None);
    // The facade and the scenery both filter by the residual; the cache
    // itself must hold nothing either.
    #[cfg(feature = "diagnostics")]
    assert_eq!(dio.diagnostics().await?.cache_rows, 0, "nothing cached");
    Ok(())
}

/// A lift-all augmentation declares no columns, so a row cached `Complete`
/// is never hydrated again: the fetched row must carry its detail already.
#[tokio::test]
async fn load_on_miss_on_a_lift_all_dio_caches_the_augmented_row() -> Result<()> {
    let detail = Vista::new("sizes", size_detail([("a", "big")]));
    let master = Vista::new(
        "files",
        MockShell::new()
            .with_metadata(meta(&["id", "kind"]))
            .with_record("a", record(&[("id", "a"), ("kind", "doc")])),
    );
    let dio = augmented_dio(
        &quick_lens(Cache::memory()),
        master,
        vec![fixed_augmentation(detail, &[])],
    )
    .await;

    let scenery = dio.record_scenery("a").load_on_miss().open().await?;
    assert!(eventually(WITHIN, || scenery.status() == RecordStatus::Fresh).await);
    let row = scenery.record().expect("the fetched row");
    assert_eq!(row.record.get("kind"), Some(&text("doc")));
    assert_eq!(
        row.record.get("size"),
        Some(&text("big")),
        "the detail is in"
    );
    Ok(())
}

#[tokio::test]
async fn an_augmented_row_reads_incomplete_until_hydrated() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let detail = Vista::new("sizes", size_detail([("a", "big")]).gated(gate.clone()));
    let master = Vista::new(
        "files",
        MockShell::new()
            .with_metadata(meta(&["id", "kind"]))
            .with_record("a", record(&[("id", "a"), ("kind", "doc")])),
    );
    let dio = augmented_dio(
        &quick_lens(Cache::memory()),
        master,
        vec![fixed_augmentation(detail, &["size"])],
    )
    .await;
    // The cached row carries the master's columns only.
    dio.apply_change(ChangeEvent::Updated {
        id: "a".into(),
        new: Some(record(&[("id", "a"), ("kind", "doc")])),
    })
    .await?;

    let scenery = dio.record_scenery("a").open().await?;
    assert_eq!(scenery.status(), RecordStatus::Incomplete);
    let row = scenery.record().expect("the partial row is shown");
    assert_eq!(row.record.get("kind"), Some(&text("doc")));
    assert_eq!(row.record.get("size"), None);

    gate.add_permits(1);
    assert!(eventually(WITHIN, || scenery.status() == RecordStatus::Fresh).await);
    let row = scenery.record().expect("the hydrated row");
    assert_eq!(row.record.get("size"), Some(&text("big")));
    Ok(())
}
