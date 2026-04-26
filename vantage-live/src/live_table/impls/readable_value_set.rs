//! Read path: cache lookup first, fall through to master on miss,
//! populate cache on the way back. Pagination state on the LiveTable
//! drives the cache-key page suffix.

use async_trait::async_trait;
use indexmap::IndexMap;
use tracing::{debug, instrument};
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, ValueSet};
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;

use crate::cache::CachedRows;
use crate::live_table::LiveTable;

#[async_trait]
impl ReadableValueSet for LiveTable {
    #[instrument(
        target = "vantage_live::read",
        skip(self),
        fields(cache_key = %self.cache_key, page)
    )]
    async fn list_values(
        &self,
    ) -> Result<IndexMap<<Self as ValueSet>::Id, Record<<Self as ValueSet>::Value>>> {
        let page = self.pagination.map(|p| p.get_page()).unwrap_or(1);
        tracing::Span::current().record("page", page);

        let key = self.page_cache_key(page);

        if let Some(cached) = self.cache.get(&key).await? {
            debug!(target: "vantage_live::read", outcome = "hit");
            return Ok(cached.rows);
        }

        debug!(target: "vantage_live::read", outcome = "miss");
        // Clone the master so concurrent readers don't fight for shared
        // mutable state. Apply pagination to the clone locally.
        let mut master = self.master.clone();
        master.set_pagination(self.pagination);
        let rows = master.list_values().await?;

        // Snapshot for cache, return original to caller.
        self.cache.put(&key, CachedRows::new(rows.clone())).await?;
        debug!(target: "vantage_live::read", outcome = "populated", rows = rows.len());
        Ok(rows)
    }

    #[instrument(
        target = "vantage_live::read",
        skip(self),
        fields(cache_key = %self.cache_key, id = %id)
    )]
    async fn get_value(
        &self,
        id: &<Self as ValueSet>::Id,
    ) -> Result<Option<Record<<Self as ValueSet>::Value>>> {
        // Single-row reads use a per-id cache slot — different shape
        // from list_values's per-page cache so they don't trample each
        // other. The shared `cache_key` prefix means a sloppy invalidate
        // wipes both at once.
        let key = self.id_cache_key(id);

        if let Some(cached) = self.cache.get(&key).await? {
            debug!(target: "vantage_live::read", outcome = "hit");
            // Cached "single row" stored as IndexMap with one entry.
            return Ok(cached.rows.into_iter().next().map(|(_, v)| v));
        }

        debug!(target: "vantage_live::read", outcome = "miss");
        let row = self.master.get_value(id).await?;

        if let Some(record) = &row {
            let mut map = IndexMap::with_capacity(1);
            map.insert(id.clone(), record.clone());
            self.cache.put(&key, CachedRows::new(map)).await?;
            debug!(target: "vantage_live::read", outcome = "populated");
        } else {
            debug!(target: "vantage_live::read", outcome = "miss_at_master");
        }
        Ok(row)
    }

    #[instrument(
        target = "vantage_live::read",
        skip(self),
        fields(cache_key = %self.cache_key)
    )]
    async fn get_some_value(
        &self,
    ) -> Result<Option<(<Self as ValueSet>::Id, Record<<Self as ValueSet>::Value>)>> {
        // Not cached: "some value" doesn't have a stable identity, so a
        // cached entry under one key wouldn't be reusable. Always pass
        // through to the master.
        self.master.get_some_value().await
    }
}
