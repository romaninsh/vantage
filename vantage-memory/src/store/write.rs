//! Table writes. Each one checks, writes the row, maintains the indexes and
//! broadcasts its change under a single hold of the write lock, so
//! concurrent writers cannot interleave between the check and the write.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::events::MemoryChange;
use super::ids::supplied_id;
use super::table::Rows;
use super::{MemoryTable, Row};

/// What `MemoryTable::upsert` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsertOutcome {
    Inserted,
    Updated,
    /// The row already held identical content; nothing was broadcast.
    Unchanged,
}

impl MemoryTable {
    fn put_new(&self, rows: &mut Rows, id: &str, row: Row) {
        rows.map.insert(id.to_string(), row.clone());
        rows.indexes.add(id, &row);
        self.changed(MemoryChange::Inserted {
            id: id.to_string(),
            row,
        });
    }

    /// Replace `old` with `row` in place. `false` when they are identical.
    fn put_over(&self, rows: &mut Rows, id: &str, old: Row, row: Row) -> bool {
        if old == row {
            return false;
        }
        if let Some(slot) = rows.map.get_mut(id) {
            *slot = row.clone();
        }
        rows.indexes.update(id, &old, &row);
        self.changed(MemoryChange::Updated {
            id: id.to_string(),
            row,
            old,
        });
        true
    }

    fn duplicate(&self, id: &str) -> vantage_core::VantageError {
        vantage_core::error!("Row already exists", table = self.name, id = id)
    }

    /// Insert a new row. The id comes from the id column when supplied,
    /// else from the table's counter. An existing id is an error.
    pub fn insert(&self, record: Record<CborValue>) -> vantage_core::Result<String> {
        let mut rows = self.rows.write();
        let id = match supplied_id(record.get(&self.def.id_column)) {
            Some(id) if rows.map.contains_key(&id) => return Err(self.duplicate(&id)),
            Some(id) => id,
            None => self.ids.next(|candidate| rows.map.contains_key(candidate)),
        };
        let row = self.with_id(record, &id);
        self.put_new(&mut rows, &id, row);
        Ok(id)
    }

    /// Insert `record` as row `id` and return the stored row. An existing
    /// id is an error.
    pub fn insert_as(&self, id: &str, record: Record<CborValue>) -> vantage_core::Result<Row> {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        if rows.map.contains_key(id) {
            return Err(self.duplicate(id));
        }
        self.put_new(&mut rows, id, row.clone());
        Ok(row)
    }

    /// Replace the existing row `id` and return the stored row. `None`, with
    /// nothing written, when the row is missing.
    pub fn replace(&self, id: &str, record: Record<CborValue>) -> Option<Row> {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        let old = rows.map.get(id)?.clone();
        self.put_over(&mut rows, id, old, row.clone());
        Some(row)
    }

    /// Insert or replace the row `id`.
    pub fn upsert(&self, id: &str, record: Record<CborValue>) -> UpsertOutcome {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        match rows.map.get(id).cloned() {
            None => {
                self.put_new(&mut rows, id, row);
                UpsertOutcome::Inserted
            }
            Some(old) => match self.put_over(&mut rows, id, old, row) {
                true => UpsertOutcome::Updated,
                false => UpsertOutcome::Unchanged,
            },
        }
    }

    /// Merge `partial` into row `id`. The id column is never changed; a
    /// value for it in `partial` is ignored. `false` when the row is missing.
    pub fn patch(&self, id: &str, partial: &Record<CborValue>) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.get(id).cloned() else {
            return false;
        };
        let mut next = (*old).clone();
        for (k, v) in partial.iter() {
            if *k != self.def.id_column {
                next.insert(k.clone(), v.clone());
            }
        }
        self.put_over(&mut rows, id, old, Arc::new(next));
        true
    }

    /// Remove row `id`. `false` when it is missing. O(n) in the table size:
    /// the row map shifts later rows down to keep insertion order.
    pub fn delete(&self, id: &str) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.shift_remove(id) else {
            return false;
        };
        rows.indexes.remove(id, &old);
        self.changed(MemoryChange::Deleted {
            id: id.to_string(),
            old,
        });
        true
    }
}
