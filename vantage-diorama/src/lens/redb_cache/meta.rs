//! Per-table meta: the stated grand total and the last refresh time.
//!
//! Meta lives in a SIBLING table (`{name}\u{1}meta`) so it can never collide
//! with a row id in the record table. Each value is eight little-endian bytes.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vantage_core::{Result, error};

use super::table::RedbCacheTable;

const TOTAL: &str = "total";
pub(super) const REFRESHED_AT: &str = "refreshed_at";

/// Marks a redb table as meta rather than a cache table of its own.
pub(super) const META_MARKER: char = '\u{1}';

/// The sibling meta table of cache table `name`.
pub(super) fn meta_table_name(name: &str) -> String {
    format!("{name}{META_MARKER}meta")
}

impl RedbCacheTable {
    fn meta_table(&self) -> String {
        meta_table_name(self.name())
    }

    async fn read_meta(&self, key: &'static str) -> Result<Option<u64>> {
        self.read(self.meta_table(), None, move |table| {
            let Some(raw) = table
                .get(key)
                .map_err(|e| error!("redb get failed", detail = e.to_string()))?
            else {
                return Ok(None);
            };
            let bytes: [u8; 8] = raw
                .value()
                .try_into()
                .map_err(|_| error!("redb meta: malformed value", key = key))?;
            Ok(Some(u64::from_le_bytes(bytes)))
        })
        .await
    }

    async fn write_meta(&self, key: &'static str, value: u64) -> Result<()> {
        self.write(self.meta_table(), move |table| {
            table
                .insert(key, value.to_le_bytes().as_slice())
                .map_err(|e| error!("redb insert failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
    }

    pub(super) async fn stored_total(&self) -> Result<Option<u64>> {
        self.read_meta(TOTAL).await
    }

    pub(super) async fn store_total(&self, total: u64) -> Result<()> {
        self.write_meta(TOTAL, total).await
    }

    /// Stored as nanoseconds since the Unix epoch, so a time round-trips
    /// exactly. A time before the epoch is refused with an error.
    pub(super) async fn stored_refreshed_at(&self) -> Result<Option<SystemTime>> {
        Ok(self
            .read_meta(REFRESHED_AT)
            .await?
            .map(|nanos| UNIX_EPOCH + Duration::from_nanos(nanos)))
    }

    pub(super) async fn store_refreshed_at(&self, at: SystemTime) -> Result<()> {
        let nanos = at
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u64::try_from(since.as_nanos()).ok())
            .ok_or_else(|| {
                error!(
                    "refresh time outside the storable range",
                    table = self.name()
                )
            })?;
        self.write_meta(REFRESHED_AT, nanos).await
    }
}
