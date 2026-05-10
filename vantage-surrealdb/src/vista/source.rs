//! `SurrealTableShell` — owns the typed `Table<SurrealDB, E>` and exposes it
//! through the `TableShell` boundary.
//!
//! The shell is generic in `E` so `with_expression` closures (parameterized
//! over `E`) survive the wrap; `Vista` erases `E` once at the
//! `Box<dyn TableShell>` boundary.
//!
//! Vista exposes ids as `String`. SurrealDB's native id is `Thing`
//! (`table:id`). The shell stringifies via `Thing::to_string()` on the way
//! out and parses back via `String::contains(':')` on the way in — bare ids
//! get prefixed with the wrapped table's name.
//!
//! `AnySurrealType` already wraps `ciborium::Value`, so the value boundary
//! is a straight unwrap/rewrap.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::operation::SurrealOperation;
use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::AnySurrealType;

pub struct SurrealTableShell<E = EmptyEntity>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) table: Table<SurrealDB, E>,
    pub(crate) capabilities: VistaCapabilities,
}

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) fn new(table: Table<SurrealDB, E>, capabilities: VistaCapabilities) -> Self {
        Self {
            table,
            capabilities,
        }
    }

    /// Resolve a Vista-side string id into a SurrealDB `Thing`. A `table:id`
    /// pair is parsed verbatim; a bare id gets prefixed with the wrapped
    /// table's name so `vista.get_value("biff")` works the same as
    /// `vista.get_value("client:biff")`.
    fn parse_id(&self, id: &str) -> Thing {
        if id.contains(':') {
            id.parse::<Thing>()
                .unwrap_or_else(|_| Thing::new(self.table.table_name(), id))
        } else {
            Thing::new(self.table.table_name(), id)
        }
    }
}

fn to_cbor_record(record: Record<AnySurrealType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnySurrealType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnySurrealType::from(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(thing, record)| (thing.to_string(), to_cbor_record(record)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let thing = self.parse_id(id);
        let Some(record) = self.table.get_value(&thing).await? else {
            return Ok(None);
        };
        Ok(Some(to_cbor_record(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((thing, record)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((thing.to_string(), to_cbor_record(record))))
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
        let thing = self.parse_id(id);
        let inserted = self
            .table
            .insert_value(&thing, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let thing = self.parse_id(id);
        let replaced = self
            .table
            .replace_value(&thing, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let thing = self.parse_id(id);
        let patched = self
            .table
            .patch_value(&thing, &to_native_record(partial))
            .await?;
        Ok(to_cbor_record(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        let thing = self.parse_id(id);
        self.table.delete(&thing).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let thing = self
            .table
            .insert_return_id_value(&to_native_record(record))
            .await?;
        Ok(thing.to_string())
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let surreal_value = AnySurrealType::from(value.clone());
        self.table.add_condition(column.eq(surreal_value));
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "surrealdb"
    }
}
