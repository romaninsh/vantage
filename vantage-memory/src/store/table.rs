//! One table: rows under a single lock, id generation and the change channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use vantage_types::Record;

use super::events::{EVENT_CAPACITY, MemoryChange};
use super::ids::{IdGen, supplied_id};
use super::{Row, TableDef};

struct Rows {
    map: IndexMap<String, Row>,
}

pub struct MemoryTable {
    name: String,
    def: TableDef,
    rows: RwLock<Rows>,
    ids: IdGen,
    events: broadcast::Sender<MemoryChange>,
    quiet: AtomicBool,
    writes: AtomicU64,
}

impl MemoryTable {
    pub(crate) fn new(name: String, def: TableDef) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            ids: IdGen::new(def.id_prefix.clone()),
            rows: RwLock::new(Rows {
                map: IndexMap::new(),
            }),
            name,
            def,
            events,
            quiet: AtomicBool::new(false),
            writes: AtomicU64::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id_column(&self) -> &str {
        &self.def.id_column
    }

    fn with_id(&self, mut record: Record<CborValue>, id: &str) -> Row {
        record.insert(self.def.id_column.clone(), CborValue::Text(id.to_string()));
        Arc::new(record)
    }

    fn changed(&self, change: MemoryChange) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if !self.quiet.load(Ordering::Relaxed) {
            let _ = self.events.send(change);
        }
    }

    /// Insert a new row. The id comes from the id column when supplied,
    /// else from the table's counter. An existing id is an error.
    pub fn insert(&self, record: Record<CborValue>) -> vantage_core::Result<String> {
        let mut rows = self.rows.write();
        let id = match supplied_id(record.get(&self.def.id_column)) {
            Some(id) if rows.map.contains_key(&id) => {
                return Err(vantage_core::error!(
                    "Row already exists",
                    table = self.name,
                    id = id
                ));
            }
            Some(id) => id,
            None => self.ids.next(|candidate| rows.map.contains_key(candidate)),
        };
        let row = self.with_id(record, &id);
        rows.map.insert(id.clone(), row.clone());
        drop(rows);
        self.changed(MemoryChange::Inserted {
            id: id.clone(),
            row,
        });
        Ok(id)
    }

    /// Insert or replace the row `id`. Replacing with an identical row is a no-op.
    pub fn upsert(&self, id: &str, record: Record<CborValue>) {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        let old = rows.map.insert(id.to_string(), row.clone());
        drop(rows);
        match old {
            None => self.changed(MemoryChange::Inserted {
                id: id.to_string(),
                row,
            }),
            Some(old) if old == row => {}
            Some(old) => self.changed(MemoryChange::Updated {
                id: id.to_string(),
                row,
                old,
            }),
        }
    }

    /// Merge `partial` into row `id`. `false` when the row is missing.
    pub fn patch(&self, id: &str, partial: &Record<CborValue>) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.get(id).cloned() else {
            return false;
        };
        let mut next = (*old).clone();
        for (k, v) in partial.iter() {
            next.insert(k.clone(), v.clone());
        }
        if next == *old {
            return true;
        }
        let row = Arc::new(next);
        rows.map.insert(id.to_string(), row.clone());
        drop(rows);
        self.changed(MemoryChange::Updated {
            id: id.to_string(),
            row,
            old,
        });
        true
    }

    /// Remove row `id`. `false` when it is missing.
    pub fn delete(&self, id: &str) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.shift_remove(id) else {
            return false;
        };
        drop(rows);
        self.changed(MemoryChange::Deleted {
            id: id.to_string(),
            old,
        });
        true
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
    pub fn set_quiet(&self, quiet: bool) {
        self.quiet.store(quiet, Ordering::Relaxed);
    }

    pub fn is_quiet(&self) -> bool {
        self.quiet.load(Ordering::Relaxed)
    }

    /// Writes that changed a row since the table was created.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MemoryChange> {
        self.events.subscribe()
    }
}
