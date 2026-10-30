//! Table writes. Each one checks, writes the row, maintains the indexes and
//! broadcasts its change under a single hold of the write lock, so
//! concurrent writers cannot interleave between the check and the write.

use std::collections::HashSet;
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

/// One row-level write, as a transaction's copy of a table records it for
/// [`MemoryTable::apply`].
pub(crate) enum Change {
    Insert(String, Row),
    Replace(String, Row),
    Upsert(String, Row),
    Patch(String, Record<CborValue>),
    Delete(String),
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

    /// Store `row` as `id`, over the row there or as a new one.
    fn put(&self, rows: &mut Rows, id: &str, row: Row) {
        match rows.map.get(id).cloned() {
            Some(old) => {
                self.put_over(rows, id, old, row);
            }
            None => self.put_new(rows, id, row),
        }
    }

    /// Record `change` when this table is a transaction's copy.
    fn journal(&self, change: impl FnOnce() -> Change) {
        if let Some(journal) = &self.journal {
            journal.lock().push(change());
        }
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
        self.put_new(&mut rows, &id, row.clone());
        self.journal(|| Change::Insert(id.clone(), row));
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
        self.journal(|| Change::Insert(id.to_string(), row.clone()));
        Ok(row)
    }

    /// Replace the existing row `id` and return the stored row. `None`, with
    /// nothing written, when the row is missing.
    pub fn replace(&self, id: &str, record: Record<CborValue>) -> Option<Row> {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        let old = rows.map.get(id)?.clone();
        self.put_over(&mut rows, id, old, row.clone());
        self.journal(|| Change::Replace(id.to_string(), row.clone()));
        Some(row)
    }

    /// Insert or replace the row `id`.
    pub fn upsert(&self, id: &str, record: Record<CborValue>) -> UpsertOutcome {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        let outcome = match rows.map.get(id).cloned() {
            None => {
                self.put_new(&mut rows, id, row.clone());
                UpsertOutcome::Inserted
            }
            Some(old) => match self.put_over(&mut rows, id, old, row.clone()) {
                true => UpsertOutcome::Updated,
                false => UpsertOutcome::Unchanged,
            },
        };
        if outcome != UpsertOutcome::Unchanged {
            self.journal(|| Change::Upsert(id.to_string(), row));
        }
        outcome
    }

    /// Merge `partial` into row `id`. The id column is never changed; a
    /// value for it in `partial` is ignored. `false` when the row is missing.
    pub fn patch(&self, id: &str, partial: &Record<CborValue>) -> bool {
        let patched = self.merge(&mut self.rows.write(), id, partial);
        if patched {
            self.journal(|| Change::Patch(id.to_string(), partial.clone()));
        }
        patched
    }

    fn merge(&self, rows: &mut Rows, id: &str, partial: &Record<CborValue>) -> bool {
        let Some(old) = rows.map.get(id).cloned() else {
            return false;
        };
        let mut next = (*old).clone();
        for (k, v) in partial.iter() {
            if *k != self.def.id_column {
                next.insert(k.clone(), v.clone());
            }
        }
        self.put_over(rows, id, old, Arc::new(next));
        true
    }

    fn take_out(&self, rows: &mut Rows, id: &str) -> bool {
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

    /// Remove row `id`. `false` when it is missing. O(n) in the table size:
    /// the row map shifts later rows down to keep insertion order.
    pub fn delete(&self, id: &str) -> bool {
        let deleted = self.take_out(&mut self.rows.write(), id);
        if deleted {
            self.journal(|| Change::Delete(id.to_string()));
        }
        deleted
    }

    /// The first id `work` inserted that this table's `rows` hold by now,
    /// other than one the block deleted before inserting it again.
    fn taken_insert(rows: &Rows, work: &MemoryTable) -> Option<String> {
        let journal = work.journal.as_ref()?.lock();
        let mut freed = HashSet::new();
        for change in journal.iter() {
            match change {
                Change::Delete(id) => {
                    freed.insert(id.as_str());
                }
                Change::Insert(id, _)
                    if !freed.remove(id.as_str()) && rows.map.contains_key(id) =>
                {
                    return Some(id.clone());
                }
                _ => {}
            }
        }
        None
    }

    /// Replay `work`'s journal onto `rows`, the rows this table holds now.
    /// Writes made here since the fork stay: a replace or a patch of a row
    /// since deleted is skipped, as an SQL `UPDATE` that matches no row,
    /// a patch merges into the current row, and rows the block never
    /// wrote are untouched.
    fn replay(&self, rows: &mut Rows, work: &MemoryTable) {
        let Some(journal) = &work.journal else {
            return;
        };
        for change in std::mem::take(&mut *journal.lock()) {
            match change {
                Change::Insert(id, row) | Change::Upsert(id, row) => self.put(rows, &id, row),
                Change::Replace(id, row) => {
                    if let Some(old) = rows.map.get(&id).cloned() {
                        self.put_over(rows, &id, old, row);
                    }
                }
                Change::Patch(id, partial) => {
                    self.merge(rows, &id, &partial);
                }
                Change::Delete(id) => {
                    self.take_out(rows, &id);
                }
            }
        }
    }

    /// Replay each `(live, work)` pair's journal, `work` a
    /// [`fork`](Self::fork) of `live`, holding every live table's write
    /// lock throughout. When an id a block inserted has been taken in its
    /// live table meanwhile, nothing is applied and the error names it.
    pub(crate) fn apply_all(pairs: &[(&MemoryTable, &MemoryTable)]) -> vantage_core::Result<()> {
        let mut locked: Vec<_> = pairs.iter().map(|(live, _)| live.rows.write()).collect();
        for ((live, work), rows) in pairs.iter().zip(&locked) {
            if let Some(id) = Self::taken_insert(rows, work) {
                return Err(vantage_core::error!(format!(
                    "\"{}\" already has a row with id \"{id}\"",
                    live.name
                )));
            }
        }
        for ((live, work), rows) in pairs.iter().zip(&mut locked) {
            live.replay(rows, work);
        }
        Ok(())
    }
}
