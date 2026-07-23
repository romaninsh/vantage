//! `GraphqlApiTableShell` — owns the typed `Table<GraphqlApi, EmptyEntity>`
//! and exposes it through the `TableShell` boundary.
//!
//! The shell speaks `AnyGraphqlType` internally (matches the underlying
//! `TableSource::Value`) and converts to/from `CborValue` at the Vista
//! boundary via the symmetric `From` impls on `AnyGraphqlType`. Vista
//! sees a uniform CBOR surface; the typed table keeps the native value
//! flow intact for filters and reference traversal.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, TableShell, Vista,
    VistaCapabilities, VistaMetadata,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::factory::GraphqlApiVistaFactory;

pub struct GraphqlApiTableShell {
    pub(crate) table: Table<GraphqlApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
}

impl GraphqlApiTableShell {
    pub(crate) fn new(
        table: Table<GraphqlApi, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }

    fn record_to_cbor(record: Record<AnyGraphqlType>) -> Record<CborValue> {
        record
            .into_iter()
            .map(|(k, v)| (k, CborValue::from(v)))
            .collect()
    }

    fn row_to_native(row: &Record<CborValue>) -> Record<AnyGraphqlType> {
        row.iter()
            .map(|(k, v)| (k.clone(), AnyGraphqlType::from(v.clone())))
            .collect()
    }
}

#[async_trait]
impl TableShell for GraphqlApiTableShell {
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
            .map(|(id, rec)| (id, Self::record_to_cbor(rec)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let Some(rec) = self.table.get_value(id).await? else {
            return Ok(None);
        };
        Ok(Some(Self::record_to_cbor(rec)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((id, rec)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((id, Self::record_to_cbor(rec))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let native = AnyGraphqlType::from(value.clone());
        let condition = Column::<AnyGraphqlType>::new(field).eq(native);
        self.table.add_condition(condition);
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // Hand-coded `with_many` / `with_one` registrations on the typed
        // table: convert the parent's CBOR row to the native value type,
        // resolve the target via `get_ref_from_row`, then re-wrap as a
        // Vista through a fresh factory bound to the same data source.
        let native_row = Self::row_to_native(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        let factory = GraphqlApiVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target::<EmptyEntity>(relation)?;
        let factory = GraphqlApiVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "graphql"
    }
}
