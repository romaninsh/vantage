use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::{Sugar, SugarFn, SugaredCache};
use crate::lens::cache_backend::{CacheBackend as _, CacheTable};
use crate::lens::memory_cache::MemoryCache;

fn rec(n: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("n".to_string(), CborValue::Integer(n.into()));
    r
}

/// Doubles `n` into `twice`, emits an undeclared `extra`, counts calls.
fn doubling(calls: Arc<AtomicUsize>) -> Sugar {
    let apply: SugarFn = Arc::new(move |rows| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            rows.into_iter()
                .map(|r| {
                    let n = r
                        .get("n")
                        .and_then(|v| v.as_integer())
                        .map(i128::from)
                        .unwrap_or(0);
                    let mut out = Record::new();
                    out.insert("twice".into(), CborValue::Integer(((n * 2) as i64).into()));
                    out.insert("extra".into(), CborValue::Bool(true));
                    Ok(out)
                })
                .collect()
        })
    });
    Sugar::new(apply, ["twice"])
}

async fn table() -> Arc<dyn CacheTable> {
    MemoryCache::new().open_table("t").await.unwrap()
}

#[tokio::test]
async fn reads_carry_outputs_and_undeclared_keys_are_dropped() {
    let inner = table().await;
    inner.insert_value("a", &rec(2)).await.unwrap();
    let sugared = SugaredCache::wrap(inner.clone(), doubling(Arc::default()));
    let row = sugared.get_value("a").await.unwrap().unwrap();
    assert_eq!(row.get("twice"), Some(&CborValue::Integer(4.into())));
    assert!(row.get("extra").is_none());
    assert!(
        inner
            .get_value("a")
            .await
            .unwrap()
            .unwrap()
            .get("twice")
            .is_none(),
        "cache untouched"
    );
}

#[tokio::test]
async fn writes_through_the_wrapper_strip_outputs() {
    let inner = table().await;
    let sugared = SugaredCache::wrap(inner.clone(), doubling(Arc::default()));
    let mut r = rec(1);
    r.insert("twice".into(), CborValue::Integer(99.into()));
    sugared.insert_value("a", &r).await.unwrap();
    assert!(
        inner
            .get_value("a")
            .await
            .unwrap()
            .unwrap()
            .get("twice")
            .is_none()
    );
}

#[tokio::test]
async fn the_memo_serves_unchanged_rows_and_recomputes_changed_ones() {
    let inner = table().await;
    inner.insert_value("a", &rec(2)).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let sugared = SugaredCache::wrap(inner.clone(), doubling(calls.clone()));
    sugared.list_values().await.unwrap();
    sugared.list_values().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "second read from the memo");
    inner.insert_value("a", &rec(5)).await.unwrap(); // a refresh behind the wrapper's back
    let row = sugared.get_value("a").await.unwrap().unwrap();
    assert_eq!(row.get("twice"), Some(&CborValue::Integer(10.into())));
}

#[tokio::test]
async fn a_failed_row_reads_without_outputs() {
    let inner = table().await;
    inner.insert_value("a", &rec(1)).await.unwrap();
    let apply: SugarFn = Arc::new(|rows| {
        Box::pin(async move {
            rows.iter()
                .map(|_| Err(vantage_core::error!("boom")))
                .collect()
        })
    });
    let sugared = SugaredCache::wrap(inner, Sugar::new(apply, ["twice"]));
    let row = sugared.get_value("a").await.unwrap().unwrap();
    assert!(row.get("twice").is_none());
    assert!(row.get("n").is_some());
}

#[tokio::test]
async fn a_failed_row_is_tried_again_on_the_next_read() {
    let inner = table().await;
    inner.insert_value("a", &rec(3)).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let apply: SugarFn = Arc::new(move |rows| {
        let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            rows.iter()
                .map(|_| {
                    if first {
                        return Err(vantage_core::error!("boom"));
                    }
                    let mut out = Record::new();
                    out.insert("twice".into(), CborValue::Integer(6.into()));
                    Ok(out)
                })
                .collect()
        })
    });
    let sugared = SugaredCache::wrap(inner, Sugar::new(apply, ["twice"]));
    sugared.get_value("a").await.unwrap();
    let row = sugared.get_value("a").await.unwrap().unwrap();
    assert_eq!(row.get("twice"), Some(&CborValue::Integer(6.into())));
}
