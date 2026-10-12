//! References: the catalog of known vistas and traversal between tables.

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use parking_lot::RwLock;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{Reference, Vista, VistaMetadata};

use super::MemoryTableShell;
use super::affinity::Untyped;
use crate::MemoryStore;

/// The store plus the metadata of every vista built over it, so a
/// traversal target gets its declared columns and references.
#[derive(Clone)]
pub struct Catalog {
    store: MemoryStore,
    entries: Arc<RwLock<HashMap<String, Entry>>>,
}

#[derive(Clone)]
struct Entry {
    metadata: VistaMetadata,
    untyped: Arc<Untyped>,
}

impl Catalog {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            entries: Arc::default(),
        }
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    /// Register `metadata` for `name`; every column's declared type counts.
    pub fn register(&self, name: impl Into<String>, metadata: VistaMetadata) {
        self.register_untyped(name, metadata, Untyped::new());
    }

    /// Register `metadata` for `name`, `untyped` naming the columns declared
    /// without a type.
    pub(crate) fn register_untyped(
        &self,
        name: impl Into<String>,
        metadata: VistaMetadata,
        untyped: Untyped,
    ) {
        let untyped = Arc::new(untyped);
        self.entries
            .write()
            .insert(name.into(), Entry { metadata, untyped });
    }

    pub fn get(&self, name: &str) -> Option<VistaMetadata> {
        self.entries.read().get(name).map(|e| e.metadata.clone())
    }

    /// An unnarrowed shell over the store table `name`, as registered;
    /// `None` when `name` has no entry.
    pub fn shell(&self, name: &str) -> Option<MemoryTableShell> {
        let entry = self.entries.read().get(name).cloned()?;
        let mut shell = MemoryTableShell::new(self.store.table(name), entry.metadata, self.clone());
        shell.untyped = entry.untyped;
        Some(shell)
    }
}

impl MemoryTableShell {
    fn reference(&self, relation: &str) -> Result<&Reference> {
        self.metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown relation", relation = relation).mark_not_found())
    }

    /// An unnarrowed shell over the reference's target table.
    fn target_shell(&self, reference: &Reference) -> MemoryTableShell {
        self.catalog.shell(&reference.target).unwrap_or_else(|| {
            let table = self.catalog.store().table(&reference.target);
            let metadata = VistaMetadata::new().with_id_column(table.id_column().to_string());
            MemoryTableShell::new(table, metadata, self.catalog.clone())
        })
    }

    /// The target of `relation`: bare for `None`, else narrowed to the rows
    /// related to `row` by the reference's join keys. Equality conditions
    /// are conformed to the stored form of their column, so an integer
    /// foreign key (as seed files usually write them) meets a text id.
    pub(super) fn traverse(
        &self,
        relation: &str,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let reference = self.reference(relation)?;
        if reference.kind.is_contained() {
            return Err(error!(
                "Contained relations are not supported by vantage-memory",
                relation = relation
            ));
        }
        let target = Vista::new(reference.target.clone(), self.target_shell(reference));
        match row {
            Some(row) => reference.narrow(target, row, CborValue::clone),
            None => Ok(target),
        }
    }
}
