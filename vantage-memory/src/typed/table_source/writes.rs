//! Patch, delete and id-assigning insert, each confined to the table's set.

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use crate::typed::MemoryDB;
use crate::typed::convert::{from_cbor_record, to_cbor_record};
use crate::types::AnyMemoryType;

impl MemoryDB {
    pub(super) async fn patch_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        partial: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        let not_found =
            || error!("Row not found", table = table.table_name(), id = id).mark_not_found();
        let store = self.store_table(table)?;
        let existing = store.get(id).ok_or_else(not_found)?;
        if !self.in_set(table, &existing).await? {
            return Err(not_found());
        }
        let id_column = self.id_column(table);
        let mut merged = (*existing).clone();
        for (k, v) in to_cbor_record(partial) {
            if k != id_column {
                merged.insert(k, v);
            }
        }
        if !self.in_set(table, &merged).await? {
            return Err(error!(
                "patch would move the row out of this set",
                table = table.table_name(),
                id = id
            )
            .mark_conflict());
        }
        if !store.patch(id, &to_cbor_record(partial)) {
            return Err(not_found());
        }
        let row = store.get(id).ok_or_else(not_found)?;
        Ok(from_cbor_record(&row))
    }

    pub(super) async fn delete_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<()> {
        let store = self.store_table(table)?;
        if let Some(row) = store.get(id)
            && self.in_set(table, &row).await?
        {
            store.delete(id);
        }
        Ok(())
    }

    pub(super) async fn delete_all_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<()> {
        let store = self.store_table(table)?;
        for (id, _) in self.rows(table, false).await? {
            store.delete(&id);
        }
        Ok(())
    }

    pub(super) async fn insert_new_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        record: &Record<AnyMemoryType>,
    ) -> Result<String> {
        let row = to_cbor_record(record);
        if !self.in_set(table, &row).await? {
            // The id is assigned by the store after this check, so none is known yet.
            return Err(error!(
                "record does not belong to this set",
                table = table.table_name()
            )
            .mark_conflict());
        }
        self.store_table(table)?.insert(row)
    }
}
