//! `RestApiTableShell` — owns the typed `Table<RestApi, EmptyEntity>`
//! and exposes it through the `TableShell` boundary.
//!
//! `RestApi` already speaks `ciborium::Value` natively (the HTTP body
//! is converted at the fetch boundary), so the shell is a pass-through
//! with no per-record translation. A clone copies the table's query state
//! (conditions, order) and shares the HTTP client.
//!
//! The shell only reads. Conditions are equality only: each becomes a URL
//! path placeholder, a query param, or an in-memory row filter, as the
//! `RestApi` is configured. A condition names a declared column or a path
//! placeholder (`teams/{team_id}/users`), which the spec needn't declare.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column as VistaColumn, Condition, FilterOp, Page, Preview,
    Reference as VistaReference, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaMetadata, unsupported,
};

use super::factory::{ModelResolver, RestApiVistaFactory};
use crate::RestApi;
use crate::rest::api::REST_DRIVER;

/// The `TableShell` behind a REST-backed `Vista`: an erased
/// `Table<RestApi, EmptyEntity>` plus the metadata and capabilities the
/// factory harvested. Built by [`RestApiVistaFactory`]; callers drive it
/// through the `Vista` it is wrapped in.
#[derive(Clone)]
pub struct RestApiTableShell {
    pub(crate) table: Table<RestApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Set on a shell built from a YAML spec: each reference's target is
    /// built by name through it. A shell wrapped from a typed table has
    /// `None` and traverses the table's own `with_one`/`with_many`.
    pub(crate) resolver: Option<ModelResolver>,
    /// The sort set by `set_order`, sent as the API's ordering query param
    /// on every windowed fetch.
    pub(crate) order: Option<(String, SortDirection)>,
}

impl RestApiTableShell {
    pub(crate) fn new(
        table: Table<RestApi, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            resolver: None,
            order: None,
        }
    }

    pub(crate) fn with_resolver(mut self, resolver: ModelResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    fn order(&self) -> Option<(&str, SortDirection)> {
        self.order
            .as_ref()
            .map(|(field, dir)| (field.as_str(), *dir))
    }

    /// The bare target of `relation`, before any join narrowing.
    fn ref_target(&self, reference: &VistaReference) -> Result<Vista> {
        if let Some(resolver) = &self.resolver {
            return resolver(&reference.target);
        }
        let target = self.table.get_ref_target_erased(&reference.name)?;
        RestApiVistaFactory::new(self.table.data_source().clone()).from_table(target)
    }
}

#[async_trait]
impl TableShell for RestApiTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        REST_DRIVER
    }

    /// The HTTP GET a read would issue — path template filled in, conditions
    /// lowered to query params, pagination applied.
    fn preview(&self) -> Preview {
        let window = self.table.pagination().map(|p| (p.skip(), p.limit()));
        self.table.data_source().preview_request(
            self.table.table_name(),
            window,
            self.order(),
            self.table.conditions(),
        )
    }

    /// The table's own read, plus the shell's order, which the table doesn't
    /// hold. A typed table's lazy expressions are applied as `list_values`
    /// would.
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let api = self.table.data_source();
        let mut rows = api.list_ordered(&self.table, self.order()).await?;
        for rec in rows.values_mut() {
            self.table.apply_lazy_expressions(rec).await?;
        }
        Ok(rows)
    }

    /// The API has no fetch-by-id endpoint the shell can rely on, so the set
    /// is listed and the row picked out.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let mut data = self.table.list_values().await?;
        Ok(data.shift_remove(id))
    }

    async fn count(&self) -> Result<i64> {
        let api = self.table.data_source();
        // Prefer the envelope total (`total_key`) — one cheap request that
        // reflects the active conditions. Falls back to counting fetched rows
        // when no `total_key` is configured.
        if let Some(total) = api
            .fetch_total(self.table.table_name(), self.table.conditions())
            .await?
        {
            return Ok(total);
        }
        api.get_table_count(&self.table).await
    }

    /// A REST envelope with a `total_key` reports the grand total on every
    /// reply, so the page carries it and a grid can size its scrollbar
    /// without a second request.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        if !self.capabilities.has(Capability::Window) {
            return unsupported(Capability::Window);
        }
        let id_field = self.table.id_field().map(|c| c.name().to_string());
        let (records, total) = self
            .table
            .data_source()
            .fetch_window_records_counted(
                self.table.table_name(),
                id_field.as_deref(),
                &crate::rest::required_fields(&self.table),
                (offset as i64, limit as i64),
                self.order(),
                self.table.conditions(),
            )
            .await?;
        // This path fetches straight from the API layer, so the table's
        // `list_values` never runs: a typed table's lazy expressions are
        // applied here. Spec `lazy:` columns are the Vista's to fill.
        let mut rows: Vec<(String, Record<CborValue>)> = records.into_iter().collect();
        for (_, rec) in rows.iter_mut() {
            self.table.apply_lazy_expressions(rec).await?;
        }
        let page = Page::new(rows);
        Ok(match total {
            Some(total) => page.with_total(total),
            None => page,
        })
    }

    /// Equality on a declared column or an endpoint `{placeholder}`, with a
    /// value that renders as one URL param. Null and compound values have no
    /// such form, so they are refused for the caller to evaluate locally.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let known = self.metadata.columns.contains_key(field)
            || crate::rest::placeholders(self.table.table_name()).any(|p| p == field);
        if !known {
            return Err(error!("Unknown column for condition", field = field));
        }
        if condition.op != FilterOp::Eq || crate::cbor_to_query_string(&condition.value).is_none() {
            return unsupported(Capability::FilterOps);
        }
        let condition = crate::eq_condition(field, condition.value.clone());
        self.table.add_condition(condition);
        Ok(())
    }

    /// One sort at a time, rendered as the API's ordering query param.
    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if !self.capabilities.has(Capability::Order) {
            return unsupported(Capability::Order);
        }
        let Some((field, dir)) = order else {
            self.order = None;
            return Ok(());
        };
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        self.order = Some((field.to_string(), dir));
        Ok(())
    }

    /// A YAML reference's target is built by name through the resolver; a
    /// typed table's comes from its `with_one`/`with_many`. From a row, the
    /// target is narrowed by the reference's join keys.
    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown relation", relation = relation).mark_not_found())?;
        let target = self.ref_target(reference)?;
        match row {
            Some(row) => reference.narrow(target, row, CborValue::clone),
            None => Ok(target),
        }
    }
}
