//! Write paths behind the shell's `TableShell` write methods.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use super::MemoryTableShell;
use crate::UpsertOutcome;

type Rec = Record<CborValue>;

impl MemoryTableShell {
    fn not_found(&self, id: &str) -> VantageError {
        error!("Row not found", table = self.table.name(), id = id)
    }

    pub(super) fn insert_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        let row = self.table.insert_as(id, record.clone())?;
        Ok((*row).clone())
    }

    pub(super) fn replace_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        let row = self
            .table
            .replace(id, record.clone())
            .ok_or_else(|| self.not_found(id))?;
        Ok((*row).clone())
    }

    pub(super) fn patch_row(&self, id: &str, partial: &Rec) -> Result<Rec> {
        if !self.table.patch(id, partial) {
            return Err(self.not_found(id));
        }
        self.table
            .get(id)
            .map(|row| (*row).clone())
            .ok_or_else(|| self.not_found(id))
    }

    pub(super) fn delete_row(&self, id: &str) -> Result<()> {
        if !self.table.delete(id) {
            return Err(self.not_found(id));
        }
        Ok(())
    }

    /// Delete every row matching the query; the window is ignored.
    pub(super) fn delete_matching(&self) -> Result<()> {
        for (id, _) in self.table.query(&self.windowed(0, None))? {
            self.table.delete(&id);
        }
        Ok(())
    }

    /// Upsert every record; returns how many ids were new.
    pub(super) fn import_rows(&self, records: &IndexMap<String, Rec>) -> usize {
        let mut inserted = 0;
        for (id, record) in records {
            if self.table.upsert(id, record.clone()) == UpsertOutcome::Inserted {
                inserted += 1;
            }
        }
        inserted
    }
}
