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
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
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
use vantage_table::traits::table_source::TableSource as _;

pub struct GraphqlApiTableShell {
    pub(crate) table: Table<GraphqlApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the quicksearch condition, so the next `add_search`
    /// replaces it rather than stacking another OR-of-ilikes on top.
    search_handle: Option<vantage_table::conditions::ConditionHandle>,
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
            search_handle: None,
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

    fn add_op_condition(
        &mut self,
        field: &str,
        op: vantage_vista::FilterOp,
        value: &CborValue,
    ) -> Result<()> {
        use vantage_vista::FilterOp;
        let column = Column::<AnyGraphqlType>::new(field);
        // Whether a given operator has a spelling is the dialect's call,
        // made at render time — Generic rejects everything but equality.
        let condition = match op {
            FilterOp::InSet | FilterOp::NotInSet => {
                let items: Vec<AnyGraphqlType> = match value {
                    CborValue::Array(items) => {
                        items.iter().cloned().map(AnyGraphqlType::from).collect()
                    }
                    single => vec![AnyGraphqlType::from(single.clone())],
                };
                if matches!(op, FilterOp::InSet) {
                    column.in_(items)
                } else {
                    column.not_in(items)
                }
            }
            scalar_op => {
                let native = AnyGraphqlType::from(value.clone());
                match scalar_op {
                    FilterOp::Eq => column.eq(native),
                    FilterOp::Ne => column.ne(native),
                    FilterOp::Gt => column.gt(native),
                    FilterOp::Gte => column.gte(native),
                    FilterOp::Lt => column.lt(native),
                    FilterOp::Lte => column.lte(native),
                    FilterOp::InSet | FilterOp::NotInSet => unreachable!("handled above"),
                }
            }
        };
        self.table.add_condition(condition);
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: vantage_vista::SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Vista's add_order is replace-semantics.
        self.table.clear_orders();
        // The table's order list is typed as conditions, and the renderer
        // reads the column name back out of the `Field` variant — so an
        // eq-condition on the column is how an order carries its field.
        // The value is never rendered.
        let expression = Column::<AnyGraphqlType>::new(field).eq(field);
        let direction = match dir {
            vantage_vista::SortDirection::Ascending => SortDirection::Ascending,
            vantage_vista::SortDirection::Descending => SortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression,
            direction,
        });
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.table.clear_orders();
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        if let Some(handle) = self.search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        let api = self.table.data_source().clone();
        let condition = api.search_table_condition(&self.table, text);
        self.search_handle = Some(self.table.temp_add_condition(condition));
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        if let Some(handle) = self.search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
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
