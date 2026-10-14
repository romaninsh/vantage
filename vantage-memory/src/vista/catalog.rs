//! [`MemoryCatalog`]: the store, plus the metadata each vista over it
//! declared.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use vantage_core::{Result, error};
use vantage_vista::{Vista, VistaMetadata, VistaResolver};

use super::MemoryTableShell;
use crate::MemoryStore;

/// The store, plus the metadata of every vista built over it.
///
/// As a [`VistaResolver`] it gives a bare Vista over a store table. A
/// registered name carries its declared columns and references, so a
/// traversal target is typed like the vista the factory built; a registered
/// name whose table is not yet in the store gets a new, empty one. A store
/// table with no registration gets only its id column. A name that is
/// neither is not-found.
///
/// Clones share the registrations.
#[derive(Clone)]
pub struct MemoryCatalog {
    store: MemoryStore,
    entries: Arc<RwLock<HashMap<String, VistaMetadata>>>,
}

impl MemoryCatalog {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            entries: Arc::default(),
        }
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    /// Register `metadata` for the store table `name`, replacing an earlier
    /// registration. A column declared without a type stores its cells as
    /// given; see [`Column::untyped`](vantage_vista::Column::untyped).
    pub fn register(&self, name: impl Into<String>, metadata: VistaMetadata) {
        self.entries.write().insert(name.into(), metadata);
    }
}

impl VistaResolver for MemoryCatalog {
    fn vista(&self, name: &str) -> Result<Vista> {
        let registered = self.entries.read().get(name).cloned();
        let (table, metadata) = match (registered, self.store.existing_table(name)) {
            (Some(metadata), table) => (table.unwrap_or_else(|| self.store.table(name)), metadata),
            (None, Some(table)) => (table, VistaMetadata::new()),
            (None, None) => {
                return Err(error!("Unknown memory table", table = name).mark_not_found());
            }
        };
        let shell = MemoryTableShell::new(table, metadata, self.clone());
        Ok(Vista::new(name, shell))
    }
}
