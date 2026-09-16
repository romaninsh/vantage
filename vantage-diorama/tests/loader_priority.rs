//! The loader, not the lens and not the transport, decides who is waiting:
//! a viewport nobody has cached is essential, a refresh over cached rows is
//! background, and the priority reaches the chunk callback through the
//! task-local even though the loader runs on its own task.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::{Priority, Result};
use vantage_diorama::Lens;
use vantage_types::Record;

mod support;
use support::MockView;
use support::chunk::master;

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn rows(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| (format!("r{i}"), rec(&format!("v{i}"))))
            .collect(),
    ))
}

/// A paged lens whose chunk callback records the priority it ran under.
fn observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
) -> Arc<Lens> {
    let total = backend.clone();
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .total_provider(move |_dio| {
                let b = total.clone();
                async move { Ok(b.lock().unwrap().len()) }
            })
            .on_load_chunk(move |_dio, range, _query, sink| {
                let b = backend.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(Priority::current());
                    let rows = b.lock().unwrap().clone();
                    for idx in range {
                        if let Some((id, r)) = rows.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    )
}

#[tokio::test]
async fn cold_open_and_uncached_viewport_are_essential_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;

    // Cold open: the cache is empty, so the on-open fetch is essential.
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    assert_eq!(seen.lock().unwrap().as_slice(), [Priority::Essential]);

    // Scrolling into rows nobody has cached: essential.
    view.viewport(20..30);
    view.settle_until("second page", |v| v.col_at(25, "v").is_some())
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));

    // A refresh over rows already on screen: background.
    view.scenery().request_refresh();
    let before = seen.lock().unwrap().len();
    view.settle_until("refresh ran", |_| seen.lock().unwrap().len() > before)
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Background));
    Ok(())
}

#[tokio::test]
async fn load_more_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before = seen.lock().unwrap().len();
    view.scenery().request_load_more();
    view.settle_until("load more ran", |_| seen.lock().unwrap().len() > before)
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

#[tokio::test]
async fn warm_open_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("c.redb");
    let backend = rows(30);
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let lens = observing_lens(cache.clone(), backend.clone(), seen);
        let dio = lens.make_dio(master(&[("v", "String")])).await?;
        let view = MockView::open(&dio, 10).await;
        view.settle_until("seeded", |v| v.loaded_rows() >= 10).await;
    }
    // Second open on the same cache file: rows are present, so the on-open
    // re-pull is a refresh, not a wait.
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(cache, backend, seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("re-pulled", |_| !seen.lock().unwrap().is_empty())
        .await;
    assert_eq!(seen.lock().unwrap().first(), Some(&Priority::Background));
    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}
