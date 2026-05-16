//! `RestApiTableShell` — owns the typed `Table<RestApi, E>` behind a
//! `dyn TableLike` so the original entity type stays attached for
//! reference traversal.
//!
//! `RestApi` already speaks `ciborium::Value` natively (the HTTP body
//! is converted at the fetch boundary), so the shell is a pass-through
//! with no per-record translation.
//!
//! Why `Box<dyn TableLike>` and not `Table<RestApi, EmptyEntity>`?
//! `with_many` / `with_one` register references whose `SourceE` is the
//! original entity (`User`, `Album`, …). At traversal time
//! `HasMany::resolve_as_any` downcasts `Table<T, SourceE>` against the
//! stored value; erasing the entity to `EmptyEntity` would break that
//! downcast.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use super::any_shell::AnyTableShell;

pub struct RestApiTableShell {
    pub(crate) table: Box<dyn TableLike<Value = CborValue, Id = String>>,
    pub(crate) capabilities: VistaCapabilities,
}

impl RestApiTableShell {
    pub(crate) fn new(
        table: Box<dyn TableLike<Value = CborValue, Id = String>>,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            table,
            capabilities,
        }
    }
}

#[async_trait]
impl TableShell for RestApiTableShell {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mut data = self.table.list_values().await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.table.list_values().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Build a typed `Expression<CborValue>` and hand it through
        // the type-erased `add_condition` API — `Table::add_condition`
        // downcasts it back to `RestApi::Condition` on the other side.
        let condition = crate::eq_condition(field, value.clone());
        self.table.add_condition(Box::new(condition))
    }

    fn get_ref(&self, relation: &str) -> Result<Vista> {
        // `TableLike::get_ref` delegates to `Table<T, E>::get_ref`,
        // which uses the typed-table reference machinery. The result
        // already carries the right conditions (parent-narrowing eq
        // translated by `related_in_condition`) and the right table
        // name (possibly a URI template, since the child factory
        // chose one). Wrap it in a Vista so generic code can keep
        // driving it.
        let any_table = self.table.get_ref(relation)?;
        AnyTableShell::into_vista(any_table)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "rest-api"
    }
}
