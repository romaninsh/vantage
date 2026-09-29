//! References: the catalog of known vistas and traversal between tables.

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use parking_lot::RwLock;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{FilterOp, Reference, ReferenceKind, Vista, VistaMetadata};

use super::MemoryTableShell;
use crate::{MemoryCondition, MemoryStore};

/// The store plus the metadata of every vista built over it, so a
/// traversal target gets its declared columns and references.
#[derive(Clone)]
pub struct Catalog {
    store: MemoryStore,
    metadata: Arc<RwLock<HashMap<String, VistaMetadata>>>,
}

impl Catalog {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            metadata: Arc::default(),
        }
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    pub fn register(&self, name: impl Into<String>, metadata: VistaMetadata) {
        self.metadata.write().insert(name.into(), metadata);
    }

    pub fn get(&self, name: &str) -> Option<VistaMetadata> {
        self.metadata.read().get(name).cloned()
    }
}

impl MemoryTableShell {
    fn reference(&self, relation: &str) -> Result<&Reference> {
        self.metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown reference", relation = relation))
    }

    /// An unnarrowed shell over the reference's target table.
    fn target_shell(&self, reference: &Reference) -> MemoryTableShell {
        let table = self.catalog.store().table(&reference.target);
        let metadata = self
            .catalog
            .get(&reference.target)
            .unwrap_or_else(|| VistaMetadata::new().with_id_column(table.id_column().to_string()));
        MemoryTableShell::new(table, metadata, self.catalog.clone())
    }

    /// The target of `relation`, narrowed to the rows related to `row`.
    /// `HasMany`: `target[foreign_key] == row[id]`.
    /// `HasOne`: `target[target id] == row[foreign_key]`.
    pub(super) fn traverse(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let reference = self.reference(relation)?;
        let mut target = self.target_shell(reference);
        let (source_col, target_col) = match reference.kind {
            ReferenceKind::HasMany => (
                self.id_column_name().to_string(),
                reference.foreign_key.clone(),
            ),
            ReferenceKind::HasOne => (
                reference.foreign_key.clone(),
                target.id_column_name().to_string(),
            ),
        };
        let value = row.get(&source_col).cloned().ok_or_else(|| {
            error!(
                "Source row is missing the join field",
                relation = relation,
                field = source_col
            )
        })?;
        target
            .query
            .conditions
            .push(MemoryCondition::cmp(target_col, FilterOp::Eq, value));
        Ok(Vista::new(reference.target.clone(), Box::new(target)))
    }

    pub(super) fn ref_target(&self, relation: &str) -> Result<Vista> {
        let reference = self.reference(relation)?;
        let target = self.target_shell(reference);
        Ok(Vista::new(reference.target.clone(), Box::new(target)))
    }
}
