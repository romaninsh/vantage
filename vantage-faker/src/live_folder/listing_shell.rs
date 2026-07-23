//! Listing `TableShell` for the live-folder sim.
//!
//! One shell per `path` reads the live tree on every `list` — no per-path
//! MockShell snapshot to keep in sync. The shell declares a `subdir` HasMany
//! reference so a Dio over a parent folder can traverse into any child:
//!
//! ```ignore
//! let ymd_dio = lens.make_dio(sim.listing_vista("ymd", "2026-07-03")).await?;
//! let row = find_row(&ymd_dio, "error_logs").await;
//! let error_logs_dio = ymd_dio.get_ref("subdir", &row).await?;
//! ```
//!
//! `get_ref("subdir", row)` reads `row[path]` (a hidden column populated by
//! `list_vista_values`) and builds a new `FolderListingShell` for that path —
//! so traversal descends one level without re-reading any other branch.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::Arc;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::capabilities::VistaCapabilities;
use vantage_vista::column::Column;
use vantage_vista::flags;
use vantage_vista::metadata::VistaMetadata;
use vantage_vista::reference::{Reference, ReferenceKind};
use vantage_vista::source::TableShell;

use super::Inner;
use super::tree::{Entry, EntryKind, format_ts};

/// Listing column set, built once and shared by every listing shell.
fn listing_metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_id_column("name")
        .with_column(
            Column::new("name", "string")
                .with_flag(flags::ID)
                .with_flag(flags::TITLE)
                .with_flag(flags::ORDERABLE),
        )
        .with_column(Column::new("kind", "string"))
        .with_column(Column::new("size", "u64").with_flag(flags::ORDERABLE))
        .with_column(Column::new("created", "datetime"))
        .with_column(Column::new("modified", "datetime").with_flag(flags::ORDERABLE))
        // Hidden helper column carrying the row's full path, so `get_ref`
        // can build the child shell without re-deriving it from `name`.
        .with_column(Column::new("path", "string").with_flag(flags::HIDDEN))
        .with_reference(Reference::new(
            "subdir",
            "live-folder-listing",
            ReferenceKind::HasMany,
            "name",
        ))
}

fn entry_to_record(path_so_far: &str, name: &str, entry: &Entry) -> Record<CborValue> {
    let full_path = if path_so_far.is_empty() {
        name.to_string()
    } else {
        format!("{path_so_far}/{name}")
    };
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(entry.name.clone()));
    r.insert(
        "kind".to_string(),
        CborValue::Text(
            match entry.kind {
                EntryKind::File => "file",
                EntryKind::Folder => "folder",
            }
            .to_string(),
        ),
    );
    // Files carry their own size; a FOLDER's recursive size is not the
    // listing's to answer — the column stays unfilled (the gap a dio-level
    // augment exists to fill from the size vista). Consumers render the
    // absence as blank, never a lying 0.
    if entry.kind == EntryKind::File {
        r.insert(
            "size".to_string(),
            CborValue::Integer((entry.size as i64).into()),
        );
    }
    r.insert(
        "created".to_string(),
        CborValue::Text(format_ts(entry.created)),
    );
    r.insert(
        "modified".to_string(),
        CborValue::Text(format_ts(entry.modified)),
    );
    r.insert("path".to_string(), CborValue::Text(full_path));
    r
}

/// `TableShell` over one folder of the live tree. Cheap to clone — shares
/// the sim's `Arc<Inner>`. The run loop just mutates the tree; this shell
/// always reads the latest state on `list`.
pub struct FolderListingShell {
    inner: Arc<Inner>,
    path: String,
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
}

impl FolderListingShell {
    pub(super) fn new(inner: Arc<Inner>, path: String) -> Self {
        Self {
            inner,
            path,
            metadata: listing_metadata(),
            capabilities: VistaCapabilities {
                can_count: true,
                can_order: true,
                can_search: true,
                ..Default::default()
            },
        }
    }
}

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for FolderListingShell {
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
        let state = self.inner.state.lock().unwrap();
        let mut rows = IndexMap::new();
        if let Some(folder) = state.tree.get(&self.path) {
            for (name, entry) in &folder.children {
                rows.insert(name.clone(), entry_to_record(&self.path, name, entry));
            }
        }
        Ok(rows)
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let state = self.inner.state.lock().unwrap();
        Ok(state
            .tree
            .get(&self.path)
            .and_then(|f| f.children.get(id))
            .map(|e| entry_to_record(&self.path, id, e)))
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Ok(self.list_vista_values(vista).await?.into_iter().next())
    }

    /// `subdir` traversal: descend one level into the child identified by
    /// `row[path]` (full path, populated by `list_vista_values`). Falls back
    /// to `self.path + "/" + row[name]` if `path` is missing.
    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        if relation != "subdir" {
            return Err(error!(
                "unknown relation",
                relation = relation,
                source_type = "FolderListingShell"
            )
            .mark_unimplemented()
            .traced());
        }
        let child_path = match row.get("path") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => {
                let name = match row.get("name") {
                    Some(CborValue::Text(s)) => s.clone(),
                    _ => {
                        return Err(error!(
                            "subdir traversal requires row.path or row.name",
                            relation = relation
                        )
                        .traced());
                    }
                };
                if self.path.is_empty() {
                    name
                } else {
                    format!("{}/{name}", self.path)
                }
            }
        };
        Ok(Vista::new(
            "live-folder-listing",
            Box::new(FolderListingShell::new(self.inner.clone(), child_path)),
        ))
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        if relation != "subdir" {
            return Err(error!(
                "unknown relation",
                relation = relation,
                source_type = "FolderListingShell"
            )
            .mark_unimplemented()
            .traced());
        }
        // The bare target of `subdir` (no parent row) is the listing rooted
        // at this shell's own path — every subdirectory that could be picked.
        Ok(Vista::new(
            "live-folder-listing",
            Box::new(FolderListingShell::new(self.inner.clone(), self.path.clone())),
        ))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(Self {
            inner: self.inner.clone(),
            path: self.path.clone(),
            metadata: self.metadata.clone(),
            capabilities: self.capabilities.clone(),
        }))
    }

    fn driver_name(&self) -> &'static str {
        "live-folder-listing"
    }
}
