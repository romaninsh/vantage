//! Custom `TableShell` for the get-only folder-size vista.
//!
//! The listing vista gives you `size = 0` on folders (size isn't part of the
//! standard listing row). To exercise viewport debounce on a slow `get`, the
//! size vista fetches a folder's recursive `(size, file_count)` with a
//! simulated latency: **100 ms base + ~0.9 ms per file, capped at 1 s**.
//! That gives a fast 100 ms on a near-empty folder and the full second on a
//! ~1000-file one — exactly the range debounce mechanics need to be tested
//! against.
//!
//! `list` is intentionally empty: this table is fetch-only. The Dio layer
//! reaches it via `on_load_detail` (one `get` per row), not via `list`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::capabilities::VistaCapabilities;
use vantage_vista::column::Column;
use vantage_vista::flags;
use vantage_vista::metadata::VistaMetadata;
use vantage_vista::reference::Reference;
use vantage_vista::source::TableShell;

use super::Inner;
use super::tree::{EntryKind, folder_size};

/// 100 ms floor for any size fetch.
const LATENCY_FLOOR: Duration = Duration::from_millis(100);
/// 1 s ceiling — large folders must not stall the UI indefinitely.
const LATENCY_CAP: Duration = Duration::from_secs(1);
/// Per-file surcharge added on top of the floor.
const LATENCY_PER_FILE_NS: u64 = 900_000; // 0.9 ms

/// `TableShell` over the simulated folder-size table.
///
/// Holds an `Arc<Inner>` so it sees the same tree the run loop mutates.
/// Cloning shares that handle — the shell is stateless beyond it.
pub struct FolderSizeShell {
    inner: Arc<Inner>,
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
}

impl FolderSizeShell {
    pub(super) fn new(inner: Arc<Inner>) -> Self {
        let metadata = VistaMetadata::new()
            .with_id_column("path")
            .with_column(
                Column::new("path", "string")
                    .with_flag(flags::ID)
                    .with_flag(flags::TITLE),
            )
            .with_column(Column::new("size", "u64").with_flag(flags::ORDERABLE))
            .with_column(Column::new("file_count", "u64").with_flag(flags::ORDERABLE));
        Self {
            inner,
            metadata,
            capabilities: VistaCapabilities {
                can_count: true,
                ..Default::default()
            },
        }
    }
}

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for FolderSizeShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        // Intentionally empty — this is a get-only table.
        Ok(IndexMap::new())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        // Compute size under the lock, then sleep *after* releasing it.
        let (size, file_count) = {
            let state = self.inner.state.lock().unwrap();
            let Some(entry) = state.tree.get(id) else {
                return Ok(None);
            };
            if entry.kind == EntryKind::File {
                // Files report their own size with no recursion.
                (entry.size, 1)
            } else {
                folder_size(entry)
            }
        };

        let per_file = Duration::from_nanos(LATENCY_PER_FILE_NS * file_count);
        let latency = (LATENCY_FLOOR + per_file).min(LATENCY_CAP);
        tokio::time::sleep(latency).await;

        Ok(Some(size_record(id, size, file_count)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Ok(None)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(Self {
            inner: self.inner.clone(),
            metadata: self.metadata.clone(),
            capabilities: self.capabilities.clone(),
        }))
    }

    fn driver_name(&self) -> &'static str {
        "live-folder-size"
    }
}

fn size_record(path: &str, size: u64, file_count: u64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("path".to_string(), CborValue::Text(path.to_string()));
    r.insert("size".to_string(), CborValue::Integer((size as i64).into()));
    r.insert(
        "file_count".to_string(),
        CborValue::Integer((file_count as i64).into()),
    );
    r
}
