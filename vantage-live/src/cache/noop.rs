//! Pass-through cache that never stores anything.
//!
//! Useful for parity tests (does the LiveTable wrapper change behaviour vs.
//! talking to the master directly?) and for opting out of caching at
//! configuration time without rebuilding the read/write path.

use async_trait::async_trait;
use vantage_core::Result;

use super::{Cache, CachedRows};

#[derive(Clone, Copy, Debug, Default)]
pub struct NoCache;

#[async_trait]
impl Cache for NoCache {
    async fn get(&self, _key: &str) -> Result<Option<CachedRows>> {
        Ok(None)
    }

    async fn put(&self, _key: &str, _rows: CachedRows) -> Result<()> {
        Ok(())
    }

    async fn invalidate_prefix(&self, _prefix: &str) -> Result<()> {
        Ok(())
    }
}
