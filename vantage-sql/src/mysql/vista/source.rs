//! `MysqlTableShell` — owns the typed `Table<MysqlDB, E>` and exposes it
//! through the `TableShell` boundary. The shell is generic in `E` so that
//! `with_expression` closures (parameterized over `E`) survive the wrap;
//! `Vista` erases `E` once at the `Box<dyn TableShell>` boundary.

use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedRefResolver, ContainedSpec, ContainedWriteback,
    Reference as VistaReference, TableShell, Vista, VistaCapabilities, VistaMetadata,
    build_contained_vista,
};

use crate::mysql::MysqlDB;
use crate::mysql::operation::MysqlOperation;
use crate::mysql::types::AnyMysqlType;
use crate::types::{cbor_to_json, parse_json_host};

pub struct MysqlTableShell<E = EmptyEntity>
where
    E: Entity<AnyMysqlType>,
{
    pub(crate) table: Table<MysqlDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
}

impl<E> MysqlTableShell<E>
where
    E: Entity<AnyMysqlType>,
{
    pub(crate) fn new(
        table: Table<MysqlDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }
}

fn to_cbor_record(record: Record<AnyMysqlType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnyMysqlType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnyMysqlType::untyped(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for MysqlTableShell<E>
where
    E: Entity<AnyMysqlType> + 'static,
{
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

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
        let sql_value = AnyMysqlType::untyped(value.clone());
        self.table.add_condition(column.eq(sql_value));
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let native_row = to_native_record(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        let factory =
            crate::mysql::vista::factory::MysqlVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target::<EmptyEntity>(relation)?;
        let factory =
            crate::mysql::vista::factory::MysqlVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        &self.metadata.contained
    }

    /// Resolve a contained relation embedded in `row`. The collection lives in
    /// the host column as JSON (a `TEXT` column round-trips as a JSON string; a
    /// native `json` column decodes to a map/array). Writes re-serialize the
    /// whole collection to JSON and `UPDATE` the column.
    fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let rel = self
            .table
            .contained_relation(relation)
            .ok_or_else(|| error!("unknown contained relation", relation = relation))?;
        let host_value = row.get(rel.host_column()).and_then(parse_json_host);

        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let parent_id = match row.get(id_field) {
            Some(CborValue::Text(s)) => s.clone(),
            Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
            _ => {
                return Err(error!(
                    "contained traversal requires the parent row's id",
                    relation = relation
                ));
            }
        };

        let contained_table = rel.build_target(self.table.data_source().clone());
        let columns: Vec<VistaColumn> =
            crate::mysql::vista::factory::metadata_from_table(&contained_table)
                .columns
                .into_values()
                .collect();
        let mut spec = ContainedSpec::new(rel.name(), rel.host_column(), rel.kind());
        if let Some(id) = rel.id_column() {
            spec = spec.with_id_column(id);
        }
        spec = spec.with_columns(columns);

        let table = self.table.clone();
        let host_column = rel.host_column().to_string();
        let writeback: ContainedWriteback = Arc::new(move |collection: CborValue| {
            let table = table.clone();
            let host_column = host_column.clone();
            let parent_id = parent_id.clone();
            Box::pin(async move {
                let json = cbor_to_json(collection).to_string();
                let mut patch: Record<AnyMysqlType> = Record::new();
                patch.insert(host_column, AnyMysqlType::untyped(CborValue::Text(json)));
                table.patch_value(&parent_id, &patch).await?;
                Ok(())
            })
        });

        let factory_db = self.table.data_source().clone();
        let ref_resolver: ContainedRefResolver =
            Arc::new(move |relation: &str, child_row: &Record<CborValue>| {
                let native = to_native_record(child_row);
                let target = contained_table.get_ref_from_row::<EmptyEntity>(relation, &native)?;
                crate::mysql::vista::factory::MysqlVistaFactory::new(factory_db.clone())
                    .from_table(target)
            });

        build_contained_vista(&spec, host_value.as_ref(), writeback, Some(ref_resolver))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "mysql"
    }
}
