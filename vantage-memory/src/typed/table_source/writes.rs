//! Insert, replace, patch and delete, each confined to the table's set. The
//! checks read first; the mutation itself goes through [`MemoryDB::write`].

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use crate::store::Row;
use crate::typed::MemoryDB;
use crate::typed::convert::{from_cbor_record, to_cbor_record};
use crate::types::AnyMemoryType;

impl MemoryDB {
    /// Insert `record` as `id` and return the stored row. An id already held
    /// by a row in the set returns that row; one held outside it is a
    /// `Conflict`.
    pub(super) async fn insert_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        if let Some(existing) = self.row_in_set(table, id).await? {
            return Ok(from_cbor_record(&existing));
        }
        let row = self.row_for(table, id, record);
        if self.fit(table, id, Row::new(row.clone())).await?.is_none() {
            return Err(Self::misfit(table.table_name(), id));
        }
        // The store's refusal stays inner: losing the race below is no failure.
        match self
            .write(table, |store| Ok(store.insert_as(id, row)))
            .await?
        {
            Ok(stored) => self.returned(table, id, stored).await,
            // Lost a race to another writer: take whatever it stored.
            Err(e) => match self.row_in_set(table, id).await? {
                Some(existing) => Ok(from_cbor_record(&existing)),
                None => Err(e),
            },
        }
    }

    /// Replace row `id` with `record`, creating it when missing.
    pub(super) async fn replace_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        let existing = self.row_in_set(table, id).await?;
        let row = self.row_for(table, id, record);
        if self.fit(table, id, Row::new(row.clone())).await?.is_none() {
            return Err(Self::misfit(table.table_name(), id));
        }
        let stored = self
            .write(table, |store| {
                match existing.and_then(|_| store.replace(id, row.clone())) {
                    Some(stored) => Ok(stored),
                    None => store.insert_as(id, row),
                }
            })
            .await?;
        self.returned(table, id, stored).await
    }

    pub(super) async fn patch_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        partial: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        let name = table.table_name().to_string();
        let not_found = move || error!("Row not found", table = name, id = id).mark_not_found();
        let existing = self
            .store_table(table)
            .await?
            .get(id)
            .ok_or_else(&not_found)?;
        // The filled row, so `merged` carries the computed roll-ups.
        let existing = self
            .fit(table, id, existing)
            .await?
            .ok_or_else(&not_found)?;
        let id_column = self.id_column(table);
        let mut merged = (*existing).clone();
        for (k, v) in to_cbor_record(partial) {
            if k != id_column && !self.is_rollup(table.table_name(), &k) {
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
        let partial = to_cbor_record(partial);
        let row = self
            .write(table, |store| match store.patch(id, &partial) {
                true => store.get(id).ok_or_else(&not_found),
                false => Err(not_found()),
            })
            .await?;
        self.returned(table, id, row).await
    }

    pub(super) async fn delete_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<()> {
        let Some(row) = self.store_table(table).await?.get(id) else {
            return Ok(());
        };
        if self.fit(table, id, row).await?.is_some() {
            self.write(table, |store| Ok(store.delete(id))).await?;
        }
        Ok(())
    }

    pub(super) async fn delete_all_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<()> {
        let rows = self.rows(table, false).await?;
        self.write(table, |store| {
            for (id, _) in &rows {
                store.delete(id);
            }
            Ok(())
        })
        .await
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
        self.write(table, |store| store.insert(row)).await
    }
}
