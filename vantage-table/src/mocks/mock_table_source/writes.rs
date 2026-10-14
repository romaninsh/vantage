//! By-id writes held to the write contract: confined to the table's
//! literal-equality conditions, and idempotent where the contract says so.

use serde_json::Value;
use vantage_dataset::{
    InsertableValueSet, ReadableValueSet, WritableValueSet, im::ImTable, traits::Result,
};
use vantage_types::{Entity, Record};

use super::MockTableSource;
use crate::table::Table;

// Ids are `&String`, the `TableSource::Id` the trait methods hand over.
#[allow(clippy::ptr_arg)]
impl MockTableSource {
    pub(super) async fn insert_row<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        id: &String,
        record: &Record<Value>,
    ) -> Result<Record<Value>> {
        Self::ensure_confinable(table)?;
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        if let Some(existing) = im_table.get_value(id).await? {
            if Self::row_in_set(table, &existing)? {
                return Ok(existing);
            }
            return Err(Self::outside(id));
        }

        let mut full = record.clone();
        full.insert("id".to_string(), Value::String(id.clone()));
        if !Self::row_in_set(table, &full)? {
            return Err(Self::misfit(id));
        }
        im_table.replace_value(id, &full).await
    }

    pub(super) async fn replace_row<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        id: &String,
        record: &Record<Value>,
    ) -> Result<Record<Value>> {
        Self::ensure_confinable(table)?;
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        if let Some(existing) = im_table.get_value(id).await?
            && !Self::row_in_set(table, &existing)?
        {
            return Err(Self::outside(id));
        }

        let mut full = record.clone();
        full.insert("id".to_string(), Value::String(id.clone()));
        if !Self::row_in_set(table, &full)? {
            return Err(Self::misfit(id));
        }
        im_table.replace_value(id, &full).await
    }

    pub(super) async fn patch_row<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        id: &String,
        partial: &Record<Value>,
    ) -> Result<Record<Value>> {
        Self::ensure_confinable(table)?;
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        let mut merged = match im_table.get_value(id).await? {
            Some(existing) if Self::row_in_set(table, &existing)? => existing,
            _ => {
                return Err(
                    vantage_core::error!("Record not found", id = id.as_str()).mark_not_found()
                );
            }
        };
        for (column, value) in partial.iter() {
            merged.insert(column.clone(), value.clone());
        }
        if !Self::row_in_set(table, &merged)? {
            return Err(vantage_core::error!(
                "patch would move the row out of this set",
                id = id.as_str()
            )
            .mark_conflict());
        }
        im_table.patch_value(id, partial).await
    }

    pub(super) async fn delete_row<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        id: &String,
    ) -> Result<()> {
        Self::ensure_confinable(table)?;
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        if let Some(existing) = im_table.get_value(id).await?
            && Self::row_in_set(table, &existing)?
        {
            im_table.delete(id).await?;
        }
        Ok(())
    }

    pub(super) async fn delete_rows<E: Entity<Value>>(&self, table: &Table<Self, E>) -> Result<()> {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        if table.conditions().next().is_none() {
            return im_table.delete_all().await;
        }
        Self::ensure_confinable(table)?;
        for (id, row) in im_table.list_values().await? {
            if Self::row_in_set(table, &row)? {
                im_table.delete(&id).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn insert_row_return_id<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Value>,
    ) -> Result<String> {
        Self::ensure_confinable(table)?;
        if !Self::row_in_set(table, record)? {
            return Err(Self::misfit("(new)"));
        }
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.insert_return_id_value(record).await
    }
}
