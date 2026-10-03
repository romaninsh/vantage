//! One table: rows under a single lock, id generation and the change channel.
//! The write operations live in `write.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use vantage_types::Record;

use super::events::{EVENT_CAPACITY, MemoryChange};
use super::ids::IdGen;
use super::index::Indexes;
use super::{Row, TableDef};

pub(super) struct Rows {
    pub(super) map: IndexMap<String, Row>,
    pub(super) indexes: Indexes,
}

pub struct MemoryTable {
    pub(super) name: String,
    pub(super) def: TableDef,
    pub(super) rows: RwLock<Rows>,
    pub(super) ids: IdGen,
    events: broadcast::Sender<MemoryChange>,
    quiet: AtomicBool,
    /// Set when a write happens while quiet; cleared by `set_quiet(false)`,
    /// which then sends `Reset`.
    missed: AtomicBool,
    writes: AtomicU64,
}

impl MemoryTable {
    pub(crate) fn new(name: String, def: TableDef) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            ids: IdGen::new(def.id_prefix.clone()),
            rows: RwLock::new(Rows {
                map: IndexMap::new(),
                indexes: Indexes::new(&def.indexed),
            }),
            name,
            def,
            events,
            quiet: AtomicBool::new(false),
            missed: AtomicBool::new(false),
            writes: AtomicU64::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id_column(&self) -> &str {
        &self.def.id_column
    }

    pub(super) fn with_id(&self, mut record: Record<CborValue>, id: &str) -> Row {
        record.insert(self.def.id_column.clone(), CborValue::Text(id.to_string()));
        Arc::new(record)
    }

    /// Count a write and broadcast it, unless the table is quiet.
    pub(super) fn changed(&self, change: MemoryChange) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if self.quiet.load(Ordering::Relaxed) {
            self.missed.store(true, Ordering::Relaxed);
        } else {
            let _ = self.events.send(change);
        }
    }

    pub fn get(&self, id: &str) -> Option<Row> {
        self.rows.read().map.get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.rows.read().map.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.rows.read().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Stop (or resume) broadcasting changes. Writes still apply and count.
    /// Resuming after writes were withheld sends one `MemoryChange::Reset`.
    pub fn set_quiet(&self, quiet: bool) {
        self.quiet.store(quiet, Ordering::Relaxed);
        if !quiet && self.missed.swap(false, Ordering::Relaxed) {
            let _ = self.events.send(MemoryChange::Reset);
        }
    }

    pub fn is_quiet(&self) -> bool {
        self.quiet.load(Ordering::Relaxed)
    }

    /// Writes that changed a row since the table was created.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    /// Add a hash index on `column`, covering every existing row. A no-op
    /// when the column is already indexed.
    pub fn add_index(&self, column: &str) {
        let mut rows = self.rows.write();
        let Rows { map, indexes } = &mut *rows;
        indexes.add_column(column, map.iter().map(|(id, row)| (id, &**row)));
    }

    pub fn is_indexed(&self, column: &str) -> bool {
        self.rows.read().indexes.has(column)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MemoryChange> {
        self.events.subscribe()
    }
}
