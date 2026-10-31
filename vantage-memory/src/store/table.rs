//! One table: rows under a single lock, id generation and the change channel.
//! The write operations live in `write.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use tokio::sync::broadcast;
use vantage_types::Record;

use super::events::{EVENT_CAPACITY, MemoryChange};
use super::ids::IdGen;
use super::index::Indexes;
use super::write::Change;
use super::{Row, TableDef};

pub(super) struct Rows {
    pub(super) map: IndexMap<String, Row>,
    pub(super) indexes: Indexes,
}

pub struct MemoryTable {
    pub(super) name: String,
    pub(super) def: TableDef,
    pub(super) rows: RwLock<Rows>,
    /// Shared with the table's transaction copies, so an id generated in
    /// one is never generated again here.
    pub(super) ids: Arc<IdGen>,
    /// On a transaction's copy only: its writes, replayed at commit.
    pub(super) journal: Option<Mutex<Vec<Change>>>,
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
            ids: Arc::new(IdGen::new(def.id_prefix.clone())),
            journal: None,
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

    /// A private, unindexed copy of the rows for a transaction to work on.
    /// It records its writes, for [`apply`](Self::apply) to replay at
    /// commit, and generates ids from this table's counter.
    pub(crate) fn fork(&self) -> MemoryTable {
        let def = TableDef {
            indexed: Vec::new(),
            ..self.def.clone()
        };
        let mut copy = MemoryTable::new(self.name.clone(), def);
        copy.ids = self.ids.clone();
        copy.journal = Some(Mutex::default());
        copy.rows.get_mut().map = self.rows.read().map.clone();
        copy
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
    ///
    /// Holds the rows write lock across the toggle and the `Reset` send, like
    /// `changed()`, so a write is never missed by the toggle nor stranded after it.
    pub fn set_quiet(&self, quiet: bool) {
        let _guard = self.rows.write();
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
