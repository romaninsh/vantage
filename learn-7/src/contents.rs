use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};

use vantage_aws::prelude::{CborValueExt, VantageResult};
use vantage_diorama::prelude::*;
use vantage_vista::prelude::*;

/// A key-value cache for downloaded file contents with **lazy admission**:
/// a file must be requested twice before its contents earn a slot. A miss
/// downloads either way; the first sight of a key only records it, a repeat
/// request stores the body, and every request after that is a cache hit.
/// One-off requests never bloat the cache — at ~122k stations, caching every
/// casually-viewed CSV would grow the file by gigabytes for nothing.
pub struct ContentsCache {
    table: Arc<dyn CacheTable>,
    /// Keys downloaded at least once — the admission ledger.
    seen: Mutex<HashSet<String>>,
}

impl ContentsCache {
    pub fn new(table: Arc<dyn CacheTable>) -> Arc<Self> {
        Arc::new(Self {
            table,
            seen: Mutex::new(HashSet::new()),
        })
    }

    /// Cache-first read: a hit is served from storage; a miss runs `fetch`,
    /// and the body is admitted only when this key was fetched before.
    pub async fn get_or_fetch<F, Fut>(&self, key: &str, fetch: F) -> VantageResult<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = VantageResult<String>>,
    {
        if let Some(row) = self.table.get_value(key).await?
            && let Some(body) = row.get("contents").and_then(|v| v.as_str())
        {
            return Ok(body.to_string());
        }
        let body = fetch().await?;
        let repeat = !self.seen.lock().unwrap().insert(key.to_string());
        if repeat {
            let record = [("contents".to_string(), CborValue::from(body.clone()))]
                .into_iter()
                .collect();
            self.table.insert_value(key, &record).await?;
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use vantage_diorama::MemoryCache;

    use super::*;

    /// The admission policy in three requests: download, download + admit,
    /// cache hit.
    #[tokio::test]
    async fn contents_cache_admits_on_second_request() {
        let backend = MemoryCache::new();
        let table = backend.open_table("contents").await.unwrap();
        let cache = ContentsCache::new(table);
        let fetches = AtomicUsize::new(0);

        for expected_fetches in [1, 2, 2] {
            let body = cache
                .get_or_fetch("GM000001153.csv", || async {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Ok("ID,DATE\nGM1,19911231".to_string())
                })
                .await
                .unwrap();
            assert_eq!(body, "ID,DATE\nGM1,19911231");
            assert_eq!(fetches.load(Ordering::SeqCst), expected_fetches);
        }
    }

    /// Different keys keep independent ledgers.
    #[tokio::test]
    async fn one_off_requests_are_not_cached() {
        let backend = MemoryCache::new();
        let table = backend.open_table("contents").await.unwrap();
        let cache = ContentsCache::new(table.clone());

        let _ = cache
            .get_or_fetch("one-off.csv", || async { Ok("data".to_string()) })
            .await
            .unwrap();
        assert!(
            table.get_value("one-off.csv").await.unwrap().is_none(),
            "a single request leaves nothing in the cache"
        );
    }
}
