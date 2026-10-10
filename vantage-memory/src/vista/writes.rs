//! Write paths behind the shell's `TableShell` write methods. Every write is
//! confined to the shell's query and fills its literal equality conditions.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_dataset::invariants::{conform, validate};
use vantage_types::Record;

use super::MemoryTableShell;
use crate::eval::matches_all;

type Rec = Record<CborValue>;

impl MemoryTableShell {
    fn not_found(&self, id: &str) -> VantageError {
        error!("Row not found", table = self.table.name(), id = id).mark_not_found()
    }

    fn in_set(&self, row: &Rec) -> Result<bool> {
        matches_all(&self.query, row)
    }

    fn misfit(&self, id: &str) -> VantageError {
        error!(
            "record does not belong to this set",
            table = self.table.name(),
            id = id
        )
        .mark_conflict()
    }

    fn outside(&self, id: &str) -> VantageError {
        error!(
            "id is held by a row outside this set",
            table = self.table.name(),
            id = id
        )
        .mark_conflict()
    }

    /// `record` filled with the set's equality conditions and `id`; a
    /// `Conflict` when the result is not in the set.
    fn conformed(&self, id: &str, record: &Rec) -> Result<Rec> {
        let mut row = record.clone();
        conform(&mut row, &self.invariants)?;
        row.insert(
            self.id_column_name().to_string(),
            CborValue::Text(id.into()),
        );
        if !self.in_set(&row)? {
            return Err(self.misfit(id));
        }
        Ok(row)
    }

    /// The stored row `id` when it is in the set, `None` when absent, and
    /// `outside` when a row outside the set holds the id.
    fn stored_in_set(&self, id: &str) -> Result<Option<Rec>> {
        let Some(existing) = self.table.get(id) else {
            return Ok(None);
        };
        match self.in_set(&existing)? {
            true => Ok(Some((*existing).clone())),
            false => Err(self.outside(id)),
        }
    }

    pub(super) fn insert_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        if let Some(existing) = self.stored_in_set(id)? {
            return Ok(existing);
        }
        let row = self.conformed(id, record)?;
        match self.table.insert_as(id, row) {
            Ok(stored) => Ok((*stored).clone()),
            // Lost a race to another writer: take whatever it stored.
            Err(e) => self.stored_in_set(id)?.ok_or(e),
        }
    }

    /// Replace row `id`, creating it when missing.
    pub(super) fn replace_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        let existing = self.stored_in_set(id)?;
        let row = self.conformed(id, record)?;
        let stored = match existing.and_then(|_| self.table.replace(id, row.clone())) {
            Some(stored) => stored,
            None => self.table.insert_as(id, row)?,
        };
        Ok((*stored).clone())
    }

    pub(super) fn upsert_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record)
    }

    pub(super) fn patch_row(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let existing = match self.stored_in_set(id) {
            Ok(Some(existing)) => existing,
            // A row held outside the set is invisible to this patch; any other
            // failure (an unevaluable condition) is the caller's to see.
            Ok(None) => return Err(self.not_found(id)),
            Err(e) if e.is_conflict() => return Err(self.not_found(id)),
            Err(e) => return Err(e),
        };
        validate(partial, &self.invariants)?;
        let mut merged = existing;
        for (k, v) in partial.iter() {
            if k != self.id_column_name() {
                merged.insert(k.clone(), v.clone());
            }
        }
        if !self.in_set(&merged)? {
            return Err(error!(
                "patch would move the row out of this set",
                table = self.table.name(),
                id = id
            )
            .mark_conflict());
        }
        if !self.table.patch(id, partial) {
            return Err(self.not_found(id));
        }
        self.table
            .get(id)
            .map(|row| (*row).clone())
            .ok_or_else(|| self.not_found(id))
    }

    /// Delete row `id` when it is in the set; success either way.
    pub(super) fn delete_row(&self, id: &str) -> Result<()> {
        if let Some(row) = self.table.get(id)
            && self.in_set(&row)?
        {
            self.table.delete(id);
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

    /// Insert a record under a generated or supplied id.
    pub(super) fn insert_return_id(&self, record: &Rec) -> Result<String> {
        let mut row = record.clone();
        conform(&mut row, &self.invariants)?;
        if !self.in_set(&row)? {
            return Err(self.misfit(""));
        }
        self.table.insert(row)
    }

    /// Insert every record whose id is new; returns how many were. Nothing is
    /// written unless every new record fits the set.
    pub(super) fn import_rows(&self, records: &IndexMap<String, Rec>) -> Result<usize> {
        let mut fitted = Vec::new();
        for (id, record) in records {
            if self.table.get(id).is_none() {
                fitted.push((id, self.conformed(id, record)?));
            }
        }
        let mut inserted = 0;
        for (id, row) in fitted {
            if self.table.insert_as(id, row).is_ok() {
                inserted += 1;
            }
        }
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use crate::eval::MemoryCondition;
    use crate::vista::Catalog;
    use vantage_vista::VistaMetadata;

    #[test]
    fn patch_propagates_set_check_errors_other_than_outside() {
        let store = MemoryStore::new();
        let table = store.table("item");
        table
            .insert_as(
                "a",
                [("n".to_string(), CborValue::Text("x".into()))]
                    .into_iter()
                    .collect(),
            )
            .unwrap();
        let mut shell = MemoryTableShell::new(
            table,
            VistaMetadata::new().with_id_column("id"),
            Catalog::new(store),
        );
        // A bare column reference is not a filter, so the set check cannot run.
        shell
            .query
            .conditions
            .push(MemoryCondition::Column("n".into()));

        let partial: Rec = [("n".to_string(), CborValue::Text("y".into()))]
            .into_iter()
            .collect();
        let err = shell.patch_row("a", &partial).unwrap_err();
        assert!(!err.is_not_found(), "set-check failure was masked: {err}");
    }
}
