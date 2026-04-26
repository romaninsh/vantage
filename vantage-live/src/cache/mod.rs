//! Cache backend abstraction for `LiveTable`.
//!
//! `LiveTable` only knows about [`Cache`] — concrete backends like
//! [`MemCache`], [`NoCache`], or `RedbCache` plug in via this trait. The
//! storage shape is `key → CachedRows`; keys are caller-supplied
//! `cache_key` plus a per-page suffix produced by `LiveTable`. See
//! `DESIGN.md` for the keying contract.

use async_trait::async_trait;
use indexmap::IndexMap;
use std::time::SystemTime;
use vantage_core::Result;
use vantage_types::Record;

mod mem;
mod noop;

pub use mem::MemCache;
pub use noop::NoCache;

/// A row set as stored in the cache, with the wall-clock instant the master
/// fetch completed. Fetch time isn't used in v1 (no TTL), but we keep it
/// because every backend can provide it cheaply and it's the obvious knob
/// to add later.
#[derive(Clone, Debug)]
pub struct CachedRows {
    pub rows: IndexMap<String, Record<ciborium::Value>>,
    pub fetched_at: SystemTime,
}

impl CachedRows {
    pub fn new(rows: IndexMap<String, Record<ciborium::Value>>) -> Self {
        Self {
            rows,
            fetched_at: SystemTime::now(),
        }
    }
}

/// Cache backend interface. Implementations must be safe to share across
/// the read path, the write-queue worker, and the live-event consumer.
#[async_trait]
pub trait Cache: Send + Sync {
    /// Look up `key`; `Ok(None)` is a normal miss, `Err` is reserved for
    /// real backend faults (disk error, redb panic recovery, etc.).
    async fn get(&self, key: &str) -> Result<Option<CachedRows>>;

    /// Store `rows` under `key`, replacing whatever was there.
    async fn put(&self, key: &str, rows: CachedRows) -> Result<()>;

    /// Drop every entry whose key starts with `prefix`. v1 callers always
    /// pass the bare `cache_key` — every page suffix below it goes.
    /// Implementations should accept any prefix (so finer-grained
    /// invalidation can be added later without breaking the trait).
    async fn invalidate_prefix(&self, prefix: &str) -> Result<()>;
}
