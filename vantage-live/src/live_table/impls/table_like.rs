//! `TableLike` impl — metadata accessors pass through to the master,
//! pagination state lives on `LiveTable` itself, condition mutators are
//! deliberately rejected (callers should build a fresh `LiveTable` for a
//! different conditioned view — see DESIGN.md).

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tracing::warn;
use vantage_core::{Result, error};
use vantage_expressions::AnyExpression;
use vantage_table::any::AnyTable;
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::traits::table_like::TableLike;

use crate::live_table::LiveTable;

#[async_trait]
impl TableLike for LiveTable {
    fn table_name(&self) -> &str {
        self.master.table_name()
    }

    fn table_alias(&self) -> &str {
        self.master.table_alias()
    }

    fn column_names(&self) -> Vec<String> {
        self.master.column_names()
    }

    fn id_field_name(&self) -> Option<String> {
        self.master.id_field_name()
    }

    fn title_field_names(&self) -> Vec<String> {
        self.master.title_field_names()
    }

    fn column_types(&self) -> IndexMap<String, &'static str> {
        self.master.column_types()
    }

    fn get_ref_names(&self) -> Vec<String> {
        self.master.get_ref_names()
    }

    fn add_condition(&mut self, _condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        // Conditions on a LiveTable would silently change what the master
        // returns under the existing cache_key — caller would see the new
        // result, then a stale read could still return the old. Build a
        // new LiveTable with a new cache_key for a different view.
        warn!(
            target: "vantage_live::table_like",
            cache_key = %self.cache_key,
            "add_condition called on LiveTable — rejected; build a new LiveTable for a different conditioned view"
        );
        Err(error!(
            "vantage-live: cannot add condition to a LiveTable; build a new LiveTable with a fresh cache_key for a different conditioned view"
        ))
    }

    fn temp_add_condition(&mut self, _condition: AnyExpression) -> Result<ConditionHandle> {
        Err(error!(
            "vantage-live: temp_add_condition not supported on LiveTable"
        ))
    }

    fn temp_remove_condition(&mut self, _handle: ConditionHandle) -> Result<()> {
        Err(error!(
            "vantage-live: temp_remove_condition not supported on LiveTable"
        ))
    }

    fn search_expression(&self, search_value: &str) -> Result<AnyExpression> {
        // Searching is a master concern — we just forward.
        self.master.search_expression(search_value)
    }

    fn clone_box(&self) -> Box<dyn TableLike<Value = Self::Value, Id = Self::Id>> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn as_any_ref(&self) -> &dyn std::any::Any {
        self
    }

    fn set_pagination(&mut self, pagination: Option<Pagination>) {
        self.pagination = pagination;
    }

    fn get_pagination(&self) -> Option<&Pagination> {
        self.pagination.as_ref()
    }

    async fn get_count(&self) -> Result<i64> {
        // Not cached in v1. Forward to master with current pagination
        // applied so callers see the count for what they'd actually
        // page through.
        let mut master = self.master.clone();
        master.set_pagination(self.pagination);
        master.get_count().await
    }

    fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        self.master.get_ref(relation)
    }
}

// Sanity: `LiveTable` inherits `Value = CborValue, Id = String` from its
// `ValueSet` impl, matching `AnyTable`'s shape exactly.
const _: fn() = || {
    fn assert_table_like<T: TableLike<Value = CborValue, Id = String>>() {}
    assert_table_like::<LiveTable>();
};
