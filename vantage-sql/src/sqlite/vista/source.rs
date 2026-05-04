//! `SqliteTableShell` — owns the typed `Table<SqliteDB, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! `AnySqliteType` already wraps `ciborium::Value`, so the boundary is a
//! straight unwrap/rewrap. `add_eq_condition` builds a typed
//! `Column<AnySqliteType>::eq` comparison via the `SqliteOperation` trait
//! and pushes it onto the wrapped table.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::sqlite::SqliteDB;
use crate::sqlite::operation::SqliteOperation;
use crate::sqlite::types::AnySqliteType;

pub struct SqliteTableShell {
    pub(crate) table: Table<SqliteDB, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
}

impl SqliteTableShell {
    pub(crate) fn new(
        table: Table<SqliteDB, EmptyEntity>,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            table,
            capabilities,
        }
    }
}

fn to_cbor_record(record: Record<AnySqliteType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnySqliteType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnySqliteType::untyped(v.clone())))
        .collect()
}

#[async_trait]
impl TableShell for SqliteTableShell {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let Some(record) = self.table.get_value(id).await? else {
            return Ok(None);
        };
        Ok(Some(to_cbor_record(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((id, record)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((id, to_cbor_record(record))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let inserted = self
            .table
            .insert_value(id, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let replaced = self
            .table
            .replace_value(id, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let patched = self
            .table
            .patch_value(id, &to_native_record(partial))
            .await?;
        Ok(to_cbor_record(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.table.delete(id).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.table
            .insert_return_id_value(&to_native_record(record))
            .await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let sql_value = AnySqliteType::untyped(value.clone());
        self.table.add_condition(column.eq(sql_value));
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
}
