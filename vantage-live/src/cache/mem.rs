//! In-memory cache backed by a `HashMap` under a `tokio::sync::RwLock`.
//!
//! Best fit for tests, short-lived processes, and the inner loop of UI
//! development where you want the wrapping-and-invalidation logic without
//! disk I/O.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use vantage_core::Result;

use super::{Cache, CachedRows};

/// Thread-safe `HashMap`-backed cache. Cheap to clone — the inner state is
/// `Arc`-shared.
#[derive(Clone, Default)]
pub struct MemCache {
    inner: Arc<RwLock<HashMap<String, CachedRows>>>,
}

impl MemCache {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Cache for MemCache {
    async fn get(&self, key: &str) -> Result<Option<CachedRows>> {
        Ok(self.inner.read().await.get(key).cloned())
    }

    async fn put(&self, key: &str, rows: CachedRows) -> Result<()> {
        self.inner.write().await.insert(key.to_string(), rows);
        Ok(())
    }

    async fn invalidate_prefix(&self, prefix: &str) -> Result<()> {
        self.inner
            .write()
            .await
            .retain(|k, _| !k.starts_with(prefix));
        Ok(())
    }
}
