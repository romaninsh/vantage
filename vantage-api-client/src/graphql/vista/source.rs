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
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, TableShell, Vista,
    VistaCapabilities, VistaMetadata,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::condition::FilterDialect;
use crate::graphql::impls::search::contains_pattern;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::lowering::TargetResolver;
use vantage_table::traits::table_source::TableSource as _;

/// The `TableShell` behind a GraphQL-backed `Vista`: an erased
/// `Table<GraphqlApi, EmptyEntity>` plus the metadata and capabilities the
/// factory harvested. Built by
/// [`GraphqlApiVistaFactory`](crate::GraphqlApiVistaFactory); callers
/// drive it through the `Vista` it is wrapped in.
#[derive(Clone)]
pub struct GraphqlApiTableShell {
    pub(crate) table: Table<GraphqlApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the quicksearch condition, so the next `add_search`
    /// replaces it rather than stacking another OR-of-ilikes on top.
    search_handle: Option<vantage_table::conditions::ConditionHandle>,
    /// Rows per page from `set_page_size`; `fetch_page` and `fetch_next`
    /// refuse to run until it is set. Paging itself lives in `paging.rs`.
    pub(crate) page_size: Option<usize>,
    /// Set on a shell built from a YAML spec: every reference it holds came
    /// from `references:`, and its target spec is looked up here at
    /// traversal time (see `traversal.rs`).
    pub(crate) resolver: Option<TargetResolver>,
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
            page_size: None,
            resolver: None,
        }
    }

    pub(crate) fn with_resolver(mut self, resolver: Option<TargetResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    pub(crate) fn record_to_cbor(record: Record<AnyGraphqlType>) -> Record<CborValue> {
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
            FilterOp::Eq => column.eq(AnyGraphqlType::from(value.clone())),
            FilterOp::Ne => column.ne(AnyGraphqlType::from(value.clone())),
            FilterOp::Gt => column.gt(AnyGraphqlType::from(value.clone())),
            FilterOp::Gte => column.gte(AnyGraphqlType::from(value.clone())),
            FilterOp::Lt => column.lt(AnyGraphqlType::from(value.clone())),
            FilterOp::Lte => column.lte(AnyGraphqlType::from(value.clone())),
            // Hasura spells "contains, any case" as `_ilike '%…%'`; Generic
            // has no pattern match, so the consumer evaluates it locally.
            FilterOp::Like => {
                if self.table.data_source().dialect() != FilterDialect::Hasura {
                    return Err(error!(
                        "GraphQL add_op_condition: Like has no push-down; filter locally",
                        method = "add_op_condition"
                    )
                    .mark_unimplemented());
                }
                column.ilike(contains_pattern(&vantage_vista::operand_text(value)))
            }
        };
        self.table.add_condition(condition);
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: vantage_vista::SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Hasura orders a nested field with a nested `order_by` object,
        // which the renderer doesn't produce; `run.id: asc` would be
        // rejected by the server.
        if field.contains('.') {
            return Err(
                error!("GraphQL cannot order by a dotted column", field = field).mark_unsupported(),
            );
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

    // Each paging method defers to the trait's default (an `Unsupported`
    // error) when the root field doesn't take `limit:`/`offset:`.

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        if !self.capabilities.can_set_page_size {
            return Err(self.default_error("set_page_size", "can_set_page_size"));
        }
        if size == 0 {
            return Err(error!("page size must be > 0"));
        }
        self.page_size = Some(size);
        Ok(())
    }

    async fn fetch_page(
        &self,
        _vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        if !self.capabilities.can_fetch_page {
            return Err(self.default_error("fetch_page", "can_fetch_page"));
        }
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self.require_page_size("fetch_page")?;
        self.list_paginated(Pagination::new(page as i64, size as i64))
            .await
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        if !self.capabilities.can_fetch_window {
            return Err(self.default_error("fetch_window", "can_fetch_window"));
        }
        self.list_paginated(Pagination::window(offset as i64, limit as i64))
            .await
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        if !self.capabilities.can_fetch_next {
            return Err(self.default_error("fetch_next", "can_fetch_next"));
        }
        self.fetch_next_page(token).await
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // The typed table's `with_many` / `with_one` registrations (from
        // code or from YAML `references:`) push the eq-condition: convert
        // the parent's CBOR row to the native value type, resolve the target
        // via `get_ref_from_row`, then re-wrap it as a Vista.
        // One bare target serves both the spec check and the join-value
        // coercion; `get_ref_from_row` builds its own, narrowed one.
        let (target_spec, bare) = match self.spec_target(relation)? {
            Some((spec, bare)) => (Some(spec), bare),
            None => (None, self.table.get_ref_target_erased(relation)?),
        };
        let mut native_row = Self::row_to_native(row);
        self.coerce_join_value(relation, &bare, row, &mut native_row)?;
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        self.wrap_target(target, target_spec)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target_spec = self.spec_target(relation)?.map(|(spec, _)| spec);
        let target = self.table.get_ref_target_erased(relation)?;
        self.wrap_target(target, target_spec)
    }

    fn get_ref_kinds(&self) -> Vec<(String, ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    /// Cheap: the `GraphqlApi` behind the table shares its HTTP client, and
    /// the copy owns only query state (conditions, order, search, page
    /// size).
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(self.clone()))
    }

    fn driver_name(&self) -> &'static str {
        "graphql"
    }

    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        // `GraphqlSelect::preview` is the synchronous renderer: it produces the
        // real document, and spells out any condition whose value is only known
        // at fetch time rather than resolving it. See `render()` for the
        // executed form, which differs only in those deferred branches.
        serde_json::json!({
            "driver": "graphql",
            "endpoint": self.table.data_source().endpoint(),
            "query": self.table.select().preview(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shell refuses a dotted order on its own, not only through the
    /// Vista's ORDERABLE check, so a caller reaching the shell directly
    /// can't render an `order_by` the server rejects.
    #[test]
    fn shell_refuses_ordering_a_dotted_column() {
        let api = GraphqlApi::new("https://api.test/graphql");
        let table = Table::<GraphqlApi, EmptyEntity>::new("runs", api)
            .with_id_column("id")
            .with_column_of::<String>("run.id");
        let mut shell =
            GraphqlApiTableShell::new(table, VistaCapabilities::default(), VistaMetadata::new());
        let err = shell
            .add_order("run.id", vantage_vista::SortDirection::Ascending)
            .unwrap_err();
        assert!(err.is_unsupported(), "{err}");
        assert!(shell.table.orders().next().is_none());
    }
}
