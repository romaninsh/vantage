use std::sync::Arc;

use tokio::sync::Barrier;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

use crate::{Cache, Lens, Loader};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new("items", MockShell::new().with_metadata(metadata))
}

/// Both builds miss the lookup and spawn, because each eager start waits for
/// the other; the one that registers second gets the first one's Dio back.
#[tokio::test]
async fn racing_builds_of_one_identity_end_on_one_dio() {
    let barrier = Arc::new(Barrier::new(2));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(Loader::eager(move |_cx| {
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                Ok(())
            }
        }))
        .build()
        .expect("lens builds");

    let (a, b) = tokio::join!(lens.dio(master()).build(), lens.dio(master()).build());
    let (a, b) = (a.expect("dio builds"), b.expect("dio builds"));
    assert!(Arc::ptr_eq(&a.inner, &b.inner));
}
