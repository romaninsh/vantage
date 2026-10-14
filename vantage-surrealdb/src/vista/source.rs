//! `SurrealTableShell` — owns the typed `Table<SurrealDB, E>` and exposes it
//! through the `TableShell` boundary.
//!
//! The shell is generic in `E` so `with_expression` closures (parameterized
//! over `E`) survive the wrap; `Vista` erases `E` once at the
//! `Box<dyn TableShell>` boundary. A clone copies the table's query state
//! (conditions, order, search) and shares the connection.
//!
//! Vista exposes ids as `String`. SurrealDB's native id is `Thing`
//! (`table:id`). The shell stringifies via `Thing::to_string()` on the way
//! out and parses back via `String::contains(':')` on the way in — bare ids
//! get prefixed with the wrapped table's name.
//!
//! `AnySurrealType` already wraps `ciborium::Value`, so the value boundary
//! is a straight unwrap/rewrap.

mod narrow;
mod refs;
#[cfg(feature = "rhai")]
mod scripted;
#[cfg(test)]
mod tests;
mod watch;

use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Condition, NativeCondition, Page, Preview, SortDirection, SpecResolver, TableShell, Vista,
    VistaCapabilities, VistaChangeStream, VistaMetadata,
};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::AnySurrealType;
use crate::vista::spec::SurrealVistaSpec;

#[cfg(feature = "rhai")]
pub(crate) use scripted::register_extensions;

#[derive(Clone)]
pub struct SurrealTableShell<E = EmptyEntity>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) table: Table<SurrealDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the active quicksearch condition, removed when the search
    /// is replaced or cleared.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// Spec resolver handed to the factories that build traversal targets.
    pub(crate) resolver: Option<Arc<dyn SpecResolver<SurrealVistaSpec>>>,
}

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) fn new(
        table: Table<SurrealDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        resolver: Option<Arc<dyn SpecResolver<SurrealVistaSpec>>>,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            resolver,
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

fn to_cbor_rows(
    raw: IndexMap<Thing, Record<AnySurrealType>>,
) -> impl Iterator<Item = (String, Record<CborValue>)> {
    raw.into_iter()
        .map(|(thing, record)| (thing.to_string(), to_cbor_record(record)))
}

#[async_trait]
impl<E> TableShell for SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "surrealdb"
    }

    /// The SurrealQL SELECT as it stands, with every condition and order
    /// rendered inline. A window's `LIMIT`/`START` is added at fetch time, so
    /// it is absent here.
    fn preview(&self) -> Preview {
        Preview::new().with_query(self.table.select().preview())
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(to_cbor_rows(self.table.list_values().await?).collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let record = self.table.get_value(self.parse_id(id)).await?;
        Ok(record.map(to_cbor_record))
    }

    async fn count(&self) -> Result<i64> {
        self.table.get_count().await
    }

    /// `LIMIT limit START offset`, applied to a clone so the shell's query
    /// is left as it was.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let mut window = self.table.clone();
        window.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        Ok(Page::new(
            to_cbor_rows(window.list_values().await?).collect(),
        ))
    }

    async fn watch(&self) -> Result<VistaChangeStream> {
        self.live_changes().await
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.apply_condition(condition)
    }

    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.apply_native_condition(condition)
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
        let native = to_native_record(record);
        let Some(id) = id else {
            let thing = self.table.insert_return_id_value(&native).await?;
            let stored = self.read_back(&thing).await?;
            return Ok((thing.to_string(), stored));
        };
        let thing = self.parse_id(id);
        let inserted = self.table.insert_value(thing.clone(), &native).await?;
        Ok((thing.to_string(), to_cbor_record(inserted)))
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let replaced = self
            .table
            .replace_value(self.parse_id(id), &to_native_record(record))
            .await?;
        Ok(to_cbor_record(replaced))
    }

    /// The replace statement is an `UPSERT … CONTENT` for a row outside the
    /// set, so it already creates a missing row in one operation.
    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.replace(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        let patched = self
            .table
            .patch_value(self.parse_id(id), &to_native_record(partial))
            .await?;
        Ok(to_cbor_record(patched))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(self.parse_id(id)).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.table.delete_all().await
    }

    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        self.ref_target(relation, row)
    }

    /// SurrealDB's expression vocabulary under `surreal::` (with the
    /// `surreal::me` anchor) and `self.with_condition(<expr>)`; see
    /// `register_extensions`.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        register_extensions(engine);
    }
}
