//! Write paths behind the shell's `TableShell` write methods.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;

use super::MemoryTableShell;

type Rec = Record<CborValue>;

impl MemoryTableShell {
    fn stored(&self, id: &str) -> Result<Rec> {
        self.table
            .get(id)
            .map(|row| (*row).clone())
            .ok_or_else(|| error!("Row not found", table = self.table.name(), id = id))
    }

    pub(super) fn insert_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        if self.table.get(id).is_some() {
            return Err(error!(
                "Row already exists",
                table = self.table.name(),
                id = id
            ));
        }
        self.table.upsert(id, record.clone());
        self.stored(id)
    }

    pub(super) fn replace_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.stored(id)?;
        self.table.upsert(id, record.clone());
        self.stored(id)
    }

    pub(super) fn patch_row(&self, id: &str, partial: &Rec) -> Result<Rec> {
        if !self.table.patch(id, partial) {
            return Err(error!("Row not found", table = self.table.name(), id = id));
        }
        self.stored(id)
    }

    pub(super) fn delete_row(&self, id: &str) -> Result<()> {
        if !self.table.delete(id) {
            return Err(error!("Row not found", table = self.table.name(), id = id));
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
            if self.table.get(id).is_none() {
                inserted += 1;
            }
            self.table.upsert(id, record.clone());
        }
        inserted
    }
}
