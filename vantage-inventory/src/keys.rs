//! The project-wide view validators check references against: which keys
//! exist per kind, plus named indexes that kinds contribute (for example
//! "which datasource does each table use").
//!
//! A [`KeySnapshot`] is taken once per validation pass, so every leaf in
//! that pass sees the same project.

use std::collections::{BTreeSet, HashMap};

/// Collects index entries from [`Kind::index`](crate::Kind::index).
#[derive(Debug, Default)]
pub struct IndexSink {
    indexes: HashMap<String, HashMap<String, Vec<String>>>,
}

impl IndexSink {
    /// Add `value` under `key` in the index named `index`. Values pushed
    /// under one key accumulate.
    pub fn push(&mut self, index: &str, key: impl Into<String>, value: impl Into<String>) {
        self.indexes
            .entry(index.to_string())
            .or_default()
            .entry(key.into())
            .or_default()
            .push(value.into());
    }

    /// Sort every value list, so the result does not depend on the order
    /// leaves were visited in.
    fn finish(mut self) -> HashMap<String, HashMap<String, Vec<String>>> {
        for index in self.indexes.values_mut() {
            index.values_mut().for_each(|values| values.sort());
        }
        self.indexes
    }
}

/// The keys and indexes of an inventory at one moment.
#[derive(Debug, Default, Clone)]
pub struct KeySnapshot {
    keys: HashMap<String, BTreeSet<String>>,
    indexes: HashMap<String, HashMap<String, Vec<String>>>,
}

impl KeySnapshot {
    /// A snapshot of `keys` (kind id to its keys) and the indexes pushed
    /// into `sink`. [`Inventory::snapshot`](crate::Inventory::snapshot)
    /// builds one from its leaves; a validator test can build one by hand.
    pub fn new(keys: HashMap<String, BTreeSet<String>>, sink: IndexSink) -> Self {
        Self {
            keys,
            indexes: sink.finish(),
        }
    }

    pub fn has(&self, kind: &str, key: &str) -> bool {
        self.keys.get(kind).is_some_and(|keys| keys.contains(key))
    }

    /// The index named `name`: entry key to its values, each list sorted.
    pub fn index(&self, name: &str) -> Option<&HashMap<String, Vec<String>>> {
        self.indexes.get(name)
    }

    /// Every index by name.
    pub fn indexes(&self) -> &HashMap<String, HashMap<String, Vec<String>>> {
        &self.indexes
    }
}
