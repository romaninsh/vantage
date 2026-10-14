//! `MongoTableShell` — owns the typed `Table<MongoDB, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! Each spec column carries a BSON path (`column_paths`) which the source uses
//! to walk nested documents on read, reconstruct nested documents on write,
//! and translate filters into Mongo's dot-notation form. When the path map is
//! empty (a bare collection, or a typed `from_table` with no columns), the
//! source falls back to identity passthrough on top-level keys.
//!
//! A clone copies the table's query state (conditions, order, search) and
//! shares the client.

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Condition, NativeCondition, Page, Preview, SortDirection, SpecResolver, TableShell, Vista,
    VistaCapabilities, VistaChangeStream, VistaMetadata,
};

use crate::id::MongoId;
use crate::mongodb::MongoDB;
use crate::types::AnyMongoType;
use crate::vista::spec::MongoVistaSpec;
use coerce::BsonKind;
use project::{project_record, walk_bson_path};

mod coerce;
mod conditions;
mod import;
mod keyset;
mod narrow;
mod paging;
mod preview;
mod project;
mod refs;
#[cfg(feature = "rhai")]
mod scripted;
#[cfg(test)]
mod tests;
mod watch;
mod write;

#[cfg(feature = "rhai")]
pub(crate) use scripted::register_extensions;

#[derive(Clone)]
pub struct MongoTableShell {
    pub(crate) table: Table<MongoDB, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// spec column name → BSON path (e.g. `"city"` → `["address", "city"]`).
    /// Empty map ⇒ identity passthrough (bare collections and typed
    /// `from_table` paths without columns).
    pub(crate) column_paths: IndexMap<String, Vec<String>>,
    /// Spec resolver handed to the factories that build traversal targets.
    pub(crate) resolver: Option<Arc<dyn SpecResolver<MongoVistaSpec>>>,
    /// Handle for the active quicksearch condition, removed when the search
    /// is replaced or cleared.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// The factory's live-update override, handed to the factories that build
    /// traversal targets.
    pub(crate) change_streams: Option<bool>,
    /// Columns whose table type is a BSON type the CBOR surface reads as
    /// text; writes parse their values back (see the `coerce` module).
    declared: IndexMap<String, BsonKind>,
}

impl MongoTableShell {
    pub(crate) fn new(
        table: Table<MongoDB, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        column_paths: IndexMap<String, Vec<String>>,
        resolver: Option<Arc<dyn SpecResolver<MongoVistaSpec>>>,
        change_streams: Option<bool>,
    ) -> Self {
        let declared = write::declared_kinds(&table, metadata.id_column.as_deref());
        Self {
            declared,
            change_streams,
            table,
            capabilities,
            metadata,
            column_paths,
            resolver,
            current_search_handle: None,
        }
    }

    pub(crate) fn parse_id(&self, id: &str) -> MongoId {
        MongoId::from_str(id).unwrap_or_else(|_| MongoId::String(id.to_string()))
    }

    /// Walk `column_paths` and project each spec column out of the raw doc.
    /// Identity (top-level keys → CBOR) when no paths are configured.
    fn unflatten_to_cbor(&self, record: Record<AnyMongoType>) -> Record<CborValue> {
        project_record(&self.column_paths, record)
    }

    fn to_rows(
        &self,
        raw: IndexMap<MongoId, Record<AnyMongoType>>,
    ) -> Vec<(String, Record<CborValue>)> {
        raw.into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect()
    }
}

#[async_trait]
impl TableShell for MongoTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "mongodb"
    }

    fn preview(&self) -> Preview {
        self.render_preview()
    }

    /// An ordered list breaks ties on `_id`, so it reads the same total order
    /// as the windows over it.
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let mut table = self.table.clone();
        if !keyset::effective_sort(&table).is_empty() {
            keyset::add_id_tiebreak(&mut table);
        }
        Ok(self
            .to_rows(table.list_values().await?)
            .into_iter()
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let record = self.table.get_value(self.parse_id(id)).await?;
        Ok(record.map(|r| self.unflatten_to_cbor(r)))
    }

    async fn count(&self) -> Result<i64> {
        self.table.data_source().get_table_count(&self.table).await
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.window(offset, limit).await
    }

    /// Keyset pagination; see the `keyset` module.
    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.seek(token, limit).await
    }

    /// A change stream on the collection; see the `watch` module.
    async fn watch(&self) -> Result<VistaChangeStream> {
        watch::watch(self).await
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
        let mongo_record = self.flatten_for_write(record, None)?;
        let Some(id) = id else {
            let id = self.table.insert_return_id_value(&mongo_record).await?;
            let stored = self.read_back(&id).await?;
            return Ok((id.to_string(), stored));
        };
        let mongo_id = self.parse_id(id);
        let inserted = self
            .table
            .insert_value(mongo_id.clone(), &mongo_record)
            .await?;
        Ok((mongo_id.to_string(), self.unflatten_to_cbor(inserted)))
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let stored = self.stored_row(&mongo_id).await?;
        let mongo_record = self.flatten_for_write(record, stored.as_ref())?;
        let replaced = self.table.replace_value(mongo_id, &mongo_record).await?;
        Ok(self.unflatten_to_cbor(replaced))
    }

    /// The driver's replace already creates a missing row (inside the set's
    /// confinement), so upsert is the same write.
    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.replace(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let stored = self.stored_row(&mongo_id).await?;
        let mongo_partial = self.flatten_for_write(partial, stored.as_ref())?;
        let patched = self.table.patch_value(mongo_id, &mongo_partial).await?;
        Ok(self.unflatten_to_cbor(patched))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(self.parse_id(id)).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.table.delete_all().await
    }

    /// All-or-nothing bulk insert in one transaction; see the `import` module.
    async fn import(&self, rows: &IndexMap<String, Record<CborValue>>) -> Result<usize> {
        import::import(self, rows).await
    }

    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        self.ref_target(relation, row)
    }

    /// BSON constructors (`oid`, `date`, `doc`) plus
    /// `self.with_condition(#{…})`; see `register_extensions`.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        register_extensions(engine);
    }
}
