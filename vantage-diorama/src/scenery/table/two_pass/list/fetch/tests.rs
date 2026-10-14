use std::sync::Arc;
use std::time::Duration;

use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

use crate::{Augmentation, Cache, Detail, DioEvent, Fetch, Lens, MergeRule, Source};

fn shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("size", "Integer"))
        .with_id_column("id");
    MockShell::new().with_metadata(metadata)
}

/// A first master page that fails takes the listing's settle mark with it:
/// left open, the mark would record every flash that settles for as long as
/// the index lives.
#[tokio::test]
async fn a_failed_first_master_page_closes_its_settle_mark() {
    let master = shell();
    let augmentation = Augmentation {
        detail: Detail::Fixed(Arc::new(Vista::new("sizes", shell()))),
        source: Source::Id,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: vec!["size".to_string()],
        },
    };
    let lens = Lens::builder()
        .cache(Cache::memory())
        .build()
        .expect("lens builds");
    let dio = lens
        .dio(Vista::new("items", master.clone()))
        .augment(vec![augmentation])
        .build()
        .await
        .expect("dio builds");
    master.set_fail_reads(true);
    let mut bus = dio.inner.event_bus.subscribe();

    let _view = dio.table_scenery().page_size(5).open().await;
    let failed = tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(bus.recv().await, Ok(DioEvent::LoadFailed { .. })) {}
    });
    failed.await.expect("the first page never failed");
    assert_eq!(dio.inner.pending_flashes.open_marks(), 0);
}
