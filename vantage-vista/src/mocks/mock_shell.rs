//! In-memory `TableShell` for tests and examples.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::{
    capabilities::{Capability, VistaCapabilities},
    filter::FilterOp,
    local,
    metadata::VistaMetadata,
    query::Condition,
    reference::Reference,
    sort::SortDirection,
};

mod refs;
mod store;
mod table_shell;
#[cfg(test)]
mod tests;

type Rec = Record<CborValue>;

/// An in-memory table.
///
/// The rows, the auto-id counter and the read-failure switch are shared by
/// every clone, so a handle kept before the shell went into a `Vista` sees
/// and changes the same store. The conditions, search and order are plain
/// fields: a clone carries them and narrows on its own.
///
/// Conditions take every [`FilterOp`]; search matches any text cell,
/// case-insensitively.
#[derive(Clone)]
pub struct MockShell {
    data: Arc<Mutex<IndexMap<String, Rec>>>,
    next_auto_id: Arc<Mutex<i64>>,
    fail_reads: Arc<AtomicBool>,
    filters: Vec<Condition>,
    order: Option<(String, SortDirection)>,
    search: Option<String>,
    capabilities: VistaCapabilities,
    metadata: VistaMetadata,
    /// Join targets by relation, see [`Self::with_ref_target`].
    ref_targets: IndexMap<String, MockShell>,
    /// Contained records' schemas by relation, see [`Self::with_contained`].
    contained: IndexMap<String, VistaMetadata>,
    /// See [`Self::with_id_prefix`].
    id_prefix: Option<String>,
}

impl MockShell {
    /// An empty table with every capability except `Import`, `Subscribe`,
    /// `Cursor` and `NativeConditions`.
    pub fn new() -> Self {
        Self {
            data: Arc::default(),
            next_auto_id: Arc::new(Mutex::new(1)),
            fail_reads: Arc::default(),
            filters: Vec::new(),
            order: None,
            search: None,
            capabilities: [
                Capability::Insert,
                Capability::Update,
                Capability::Delete,
                Capability::Order,
                Capability::Search,
                Capability::FilterOps,
                Capability::Window,
                Capability::Traverse,
                Capability::ConfineWrites,
            ]
            .into_iter()
            .collect(),
            metadata: VistaMetadata::new(),
            ref_targets: IndexMap::new(),
            contained: IndexMap::new(),
            id_prefix: None,
        }
    }

    pub fn with_capabilities(mut self, capabilities: VistaCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Replaces the schema, references included: call it before
    /// [`with_contained`](Self::with_contained).
    pub fn with_metadata(mut self, metadata: VistaMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// The target store of a join declared in the metadata. `get_ref`
    /// returns a clone of it, narrowed by the reference's keys against the
    /// parent row: the in-memory analogue of a driver's `with_one`/`with_many`.
    pub fn with_ref_target(mut self, relation: impl Into<String>, target: MockShell) -> Self {
        self.ref_targets.insert(relation.into(), target);
        self
    }

    /// Declare a contained relation, its records described by `schema`.
    /// `get_ref` serves it from the parent row's host column, writing changes
    /// back into this store.
    pub fn with_contained(mut self, reference: Reference, schema: VistaMetadata) -> Self {
        self.contained.insert(reference.name.clone(), schema);
        self.metadata = self.metadata.with_reference(reference);
        self
    }

    /// Qualify every id with `prefix`, the way a driver that owns its key
    /// space does: a record inserted as `abc` is stored — and returned —
    /// as `client:abc`. An id that already carries the prefix passes
    /// through, so a caller can address a row in either form.
    pub fn with_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = Some(prefix.into());
        self
    }

    /// Seed a record with an explicit id.
    pub fn with_record(self, id: impl Into<String>, record: Rec) -> Self {
        self.set_record(id, record);
        self
    }

    // ---- Helpers for the TableShell impl ------------------------------------

    fn guard_reads(&self) -> Result<()> {
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err(error!("mock source read failed (injected)"));
        }
        Ok(())
    }

    /// The store key for `id` under [`Self::with_id_prefix`].
    fn key(&self, id: &str) -> String {
        match &self.id_prefix {
            Some(prefix) if !id.starts_with(prefix.as_str()) => format!("{prefix}{id}"),
            _ => id.to_string(),
        }
    }

    fn id_field(&self) -> &str {
        self.metadata.id_column.as_deref().unwrap_or("id")
    }

    /// Whether `record` belongs to this shell's narrowed set.
    fn in_set(&self, record: &Rec) -> bool {
        self.filters.iter().all(|c| local::matches(c, record))
            && self
                .search
                .as_deref()
                .is_none_or(|needle| local::matches_search(needle, record))
    }

    /// `record` as stored under `key`: conformed to the equality filters,
    /// keyed in the id column, and required to land inside the set.
    fn fit(&self, record: &Rec, key: &str) -> Result<Rec> {
        let invariants: IndexMap<String, CborValue> = self
            .filters
            .iter()
            .filter(|c| c.op == FilterOp::Eq)
            .map(|c| (c.column.clone(), c.value.clone()))
            .collect();
        let mut stored = record.clone();
        vantage_dataset::invariants::conform(&mut stored, &invariants)?;
        stored.insert(
            self.id_field().to_string(),
            CborValue::Text(key.to_string()),
        );
        if !self.in_set(&stored) {
            return Err(error!("Record would fall outside the set", id = key).mark_conflict());
        }
        Ok(stored)
    }

    fn next_auto_id(&self) -> String {
        let mut next = self.next_auto_id.lock().unwrap();
        let id = next.to_string();
        *next += 1;
        id
    }

    /// The rows in the set, in order, borrowed from `data`.
    fn matching<'a>(&self, data: &'a IndexMap<String, Rec>) -> Vec<(&'a String, &'a Rec)> {
        let mut rows: Vec<_> = data.iter().filter(|(_, r)| self.in_set(r)).collect();
        if let Some((field, dir)) = &self.order {
            rows.sort_by(|a, b| {
                let ord = local::cbor_cmp(a.1.get(field), b.1.get(field));
                match dir {
                    SortDirection::Ascending => ord,
                    SortDirection::Descending => ord.reverse(),
                }
            });
        }
        rows
    }
}

impl Default for MockShell {
    fn default() -> Self {
        Self::new()
    }
}
