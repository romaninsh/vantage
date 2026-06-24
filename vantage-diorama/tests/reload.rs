//! Step 6: `Dio::reload` — swap the master Vista + dataset in place without
//! blanking open sceneries (the "its VistaFactory reloaded" path).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn named(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r
}

/// A master over an in-memory shell seeded with `(id, name)` rows.
fn master(rows: &[(&str, &str)]) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    let mut shell = MockShell::new().with_metadata(metadata);
    for (id, name) in rows {
        shell = shell.with_record(*id, named(name));
    }
    Vista::new("items", Box::new(shell))
}

async fn eager_lens(cache_path: std::path::PathBuf) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    )
}

fn name_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery.row(idx).and_then(|r| {
        r.record.get("name").and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
    })
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reload_swaps_dataset_without_blanking() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("cache.redb")).await;
    let dio = lens
        .make_dio(master(&[("a", "alpha"), ("b", "beta")]))
        .await?;

    let scenery = dio.table_scenery().open().await?;
    eventually("initial seed", || scenery.row_count() == 2).await;

    // Sample row_count continuously while the reload runs — a non-blanking
    // reload never lets it dip below the original 2 (the new rows swap in
    // atomically; the cache being briefly empty must not reach the view).
    let stop = Arc::new(AtomicBool::new(false));
    let min_seen = Arc::new(AtomicUsize::new(usize::MAX));
    let sampler = {
        let sc = scenery.clone();
        let stop = stop.clone();
        let min_seen = min_seen.clone();
        tokio::spawn(async move {
            while !stop.load(Ordering::Acquire) {
                min_seen.fetch_min(sc.row_count(), Ordering::SeqCst);
                tokio::task::yield_now().await;
            }
            // one final sample after the flag flips
            min_seen.fetch_min(sc.row_count(), Ordering::SeqCst);
        })
    };

    // Reload with a wholly different dataset (3 rows, new ids).
    dio.reload(master(&[("x", "ex"), ("y", "why"), ("z", "zee")]))
        .await?;
    eventually("reseed to new data", || scenery.row_count() == 3).await;

    stop.store(true, Ordering::Release);
    sampler.await.unwrap();

    // New data is in view, old ids gone.
    assert_eq!(scenery.row_count(), 3);
    assert_eq!(name_at(&scenery, 0).as_deref(), Some("ex"));
    assert!(
        (0..3).all(|i| name_at(&scenery, i) != Some("alpha".into())),
        "old rows must be gone after reload"
    );
    // The master swapped too.
    assert_eq!(dio.master().get_count().await?, 3);

    // No blank: the view never showed fewer than the original 2 rows.
    let min = min_seen.load(Ordering::SeqCst);
    assert!(
        min >= 2,
        "scenery blanked mid-reload — min row_count was {min}"
    );
    Ok(())
}
