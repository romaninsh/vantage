//! `GraphqlApiTableShell` — owns the typed `Table<GraphqlApi, EmptyEntity>`
//! and exposes it through the `TableShell` boundary.
//!
//! The shell speaks `AnyGraphqlType` internally (matches the underlying
//! `TableSource::Value`) and converts to/from `CborValue` at the Vista
//! boundary via the symmetric `From` impls on `AnyGraphqlType`. Vista
//! sees a uniform CBOR surface; the typed table keeps the native value
//! flow intact for filters and reference traversal.
//!
//! A clone copies the table's query state (conditions, order, search) and
//! shares the HTTP client. Narrowing lives in `narrow.rs`, paging in
//! `paging.rs`, traversal in `traversal.rs`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Condition, Page, Preview, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaMetadata, unsupported,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::lowering::TargetSpecResolver;

/// The shell's `driver_name`.
const DRIVER: &str = "graphql";

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
    /// Handle for the quicksearch condition, so the next `set_search`
    /// replaces it rather than stacking another OR-of-ilikes on top.
    pub(crate) search_handle: Option<ConditionHandle>,
    /// Set on a shell built from a YAML spec: every reference it holds came
    /// from `references:`, and its target spec is looked up here at
    /// traversal time (see `traversal.rs`).
    pub(crate) resolver: Option<TargetSpecResolver>,
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
            resolver: None,
        }
    }

    pub(crate) fn with_resolver(mut self, resolver: Option<TargetSpecResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    pub(crate) fn record_to_cbor(record: Record<AnyGraphqlType>) -> Record<CborValue> {
        record
            .into_iter()
            .map(|(k, v)| (k, CborValue::from(v)))
            .collect()
    }
}

#[async_trait]
impl TableShell for GraphqlApiTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        DRIVER
    }

    /// `GraphqlSelect::preview` is the synchronous renderer: it produces the
    /// real document, and spells out any condition whose value is only known
    /// at fetch time rather than resolving it. The executed form differs only
    /// in those deferred branches. The document is posted to the endpoint
    /// named on the first line.
    fn preview(&self) -> Preview {
        Preview::new().with_query(format!(
            "POST {}\n{}",
            self.table.data_source().endpoint(),
            self.table.select().preview()
        ))
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, rec)| (id, Self::record_to_cbor(rec)))
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self.table.get_value(id).await?.map(Self::record_to_cbor))
    }

    async fn count(&self) -> Result<i64> {
        self.table.get_count().await
    }

    /// `limit:`/`offset:` on a clone of the table, when the root field takes
    /// them and conditions reach the server. A GraphQL list reports no total,
    /// so the page carries none. Otherwise the leading window is cut from
    /// the listing.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        if !self.capabilities.has(Capability::Window) {
            return Ok(Page::from_listing(self.list().await?, offset, limit));
        }
        let rows = self
            .list_paginated(Pagination::window(offset as i64, limit as i64))
            .await?;
        Ok(Page::new(rows))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.apply_condition(condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        if !self.capabilities.has(Capability::Search) {
            return unsupported(Capability::Search);
        }
        self.apply_search(text)
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if !self.capabilities.has(Capability::Order) {
            return unsupported(Capability::Order);
        }
        self.apply_order(order)
    }

    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        self.ref_target(relation, row)
    }
}
