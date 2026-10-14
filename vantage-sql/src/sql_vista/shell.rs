//! `SqlTableShell` — owns the typed `Table<T, E>` of a SQL dialect and
//! exposes it through the `TableShell` boundary. The shell is generic in `E`
//! so that `with_expression` closures (parameterized over `E`) survive the
//! wrap; `Vista` erases `E` once at the `Box<dyn TableShell>` boundary. A
//! clone copies the table's query state (conditions, order, search) and
//! shares the connection pool.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Condition, NativeCondition, Page, Preview, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaChangeStream, VistaMetadata,
};

use super::dialect::SqlShellDialect;

/// The vista shell of every SQL dialect; `X` is the dialect's
/// [`Extra`](SqlShellDialect::Extra) state.
#[derive(Clone)]
pub struct SqlTableShell<T, E = EmptyEntity, X = ()>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    pub(crate) table: Table<T, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the active quicksearch condition, removed when the search
    /// is replaced or cleared.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    // Only MySQL carries extra state.
    #[cfg_attr(not(feature = "mysql"), allow(dead_code))]
    pub(crate) extra: X,
}

impl<T, E, X> SqlTableShell<T, E, X>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    pub(crate) fn new(
        table: Table<T, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        extra: X,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            extra,
        }
    }
}

pub(crate) fn cbor_record<T: SqlShellDialect>(record: Record<T::Value>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, T::cbor(v))).collect()
}

pub(crate) fn native_record<T: SqlShellDialect>(record: &Record<CborValue>) -> Record<T::Value> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), T::cell(v.clone())))
        .collect()
}

fn cbor_rows<T, C>(rows: IndexMap<String, Record<T::Value>>) -> C
where
    T: SqlShellDialect,
    C: FromIterator<(String, Record<CborValue>)>,
{
    rows.into_iter()
        .map(|(id, record)| (id, cbor_record::<T>(record)))
        .collect()
}

#[async_trait]
impl<T, E> TableShell for SqlTableShell<T, E, T::Extra>
where
    T: SqlShellDialect,
    E: Entity<T::Value> + 'static,
{
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        T::DRIVER
    }

    /// The SELECT as it stands, rendered with values inline. The executed
    /// form binds those values as the dialect's parameters instead.
    fn preview(&self) -> Preview {
        Preview::new().with_query(T::preview(&self.table))
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(cbor_rows::<T, _>(self.table.list_values().await?))
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self.table.get_value(id).await?.map(cbor_record::<T>))
    }

    async fn count(&self) -> Result<i64> {
        self.table.data_source().get_table_count(&self.table).await
    }

    /// The window applies to a clone, leaving the shell's query as it was.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let mut window = T::ordered_for_paging(&self.table);
        window.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        Ok(Page::new(cbor_rows::<T, _>(window.list_values().await?)))
    }

    async fn watch(&self) -> Result<VistaChangeStream> {
        T::watch(self)
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.apply_condition(condition)
    }

    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        T::add_native_condition(self, condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.apply_search(text)
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.apply_order(order)
    }

    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        let native = native_record::<T>(record);
        let Some(id) = id else {
            let id = self.table.insert_return_id_value(&native).await?;
            let stored = self.read_back(&id).await?;
            return Ok((id, stored));
        };
        let inserted = self.table.insert_value(id, &native).await?;
        Ok((id.to_string(), cbor_record::<T>(inserted)))
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let replaced = self
            .table
            .replace_value(id, &native_record::<T>(record))
            .await?;
        Ok(cbor_record::<T>(replaced))
    }

    /// Every dialect's replace statement (`INSERT … ON CONFLICT DO UPDATE`,
    /// `… ON DUPLICATE KEY UPDATE`, `INSERT OR REPLACE`) already creates a
    /// missing row in one operation.
    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.replace(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        let patched = self
            .table
            .patch_value(id, &native_record::<T>(partial))
            .await?;
        Ok(cbor_record::<T>(patched))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.table.delete_all().await
    }

    async fn import(&self, records: &IndexMap<String, Record<CborValue>>) -> Result<usize> {
        T::import(self, records).await
    }

    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        self.ref_target(relation, row)
    }

    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        T::register_rhai_extensions(engine);
    }
}
