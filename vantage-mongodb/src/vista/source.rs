//! `MongoTableShell` — owns the typed `Table<MongoDB, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! Each spec column carries a BSON path (`column_paths`) which the source uses
//! to walk nested documents on read, reconstruct nested documents on write,
//! and translate `eq` filters into Mongo's dot-notation form. When the path
//! map is empty (e.g. a typed `from_table` with no columns), the source falls
//! back to identity passthrough on top-level keys.

use std::str::FromStr;

use async_trait::async_trait;
use bson::{Bson, doc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedSpec, FilterOp, Reference as VistaReference, SortDirection,
    TableShell, Vista, VistaCapabilities, VistaChangeStream, VistaMetadata, flags,
};

use crate::condition::MongoCondition;
use crate::id::{MongoId, eq_filter};
use crate::mongodb::MongoDB;
use crate::mongodb::impls::joins::{join_alias_of, table_joins};
use crate::types::AnyMongoType;
use crate::vista::cbor::{bson_to_cbor, cbor_to_bson};
use crate::vista::factory::{MongoSpecResolver, MongoVistaFactory};
use coerce::BsonKind;

mod coerce;
mod conditions;
mod import;
mod keyset;
#[cfg(feature = "rhai")]
mod scripted;
mod watch;
mod write;

pub struct MongoTableShell {
    pub(crate) table: Table<MongoDB, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// spec column name → BSON path (e.g. `"city"` → `["address", "city"]`).
    /// Empty map ⇒ identity passthrough (used by typed `from_table` paths).
    pub(crate) column_paths: IndexMap<String, Vec<String>>,
    /// Spec resolver handed to the factories that build traversal targets.
    pub(crate) resolver: Option<MongoSpecResolver>,
    /// Handle for the active quicksearch condition (if any).
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// Page size declared via `set_page_size`.
    pub(crate) page_size: Option<usize>,
    /// A vista wrapping a typed table (`from_table`) searches every column
    /// when none is flagged `searchable`; a spec-built vista searches only its
    /// `searchable` columns, so none flagged matches nothing. Traversal
    /// targets (`get_ref`, `get_ref_target`) are wrapped with `from_table`
    /// even when their table came from a spec, so they search every column
    /// and run neither that spec's `modify:` script nor its reference scripts.
    pub(crate) search_unflagged_columns: bool,
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
        resolver: Option<MongoSpecResolver>,
        search_unflagged_columns: bool,
        change_streams: Option<bool>,
    ) -> Self {
        let declared = write::declared_kinds(&table, metadata.id_column.as_deref());
        Self {
            declared,
            search_unflagged_columns,
            change_streams,
            table,
            capabilities,
            metadata,
            column_paths,
            resolver,
            current_search_handle: None,
            page_size: None,
        }
    }

    /// A factory over this table's data source, carrying the spec resolver
    /// and the live-update override.
    fn factory(&self) -> MongoVistaFactory {
        let mut factory = MongoVistaFactory::new(self.table.data_source().clone());
        factory.change_streams = self.change_streams;
        match &self.resolver {
            Some(resolver) => factory.with_resolver(resolver.clone()),
            None => factory,
        }
    }

    pub(crate) fn parse_id(&self, id: &str) -> MongoId {
        MongoId::from_str(id).unwrap_or_else(|_| MongoId::String(id.to_string()))
    }

    async fn read_all(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect())
    }

    /// Walk `column_paths` and project each spec column out of the raw doc.
    /// Identity (top-level keys → CBOR) when no paths are configured.
    fn unflatten_to_cbor(&self, record: Record<AnyMongoType>) -> Record<CborValue> {
        project_record(&self.column_paths, record)
    }

    /// Quicksearch filter over the columns flagged `searchable`. A shell over
    /// a typed table that flags none searches every column instead, matching
    /// `MongoDB::search_table_condition`.
    fn search_condition(&self, text: &str) -> MongoCondition {
        let paths: Vec<String> = self
            .metadata
            .columns
            .iter()
            .filter(|(_, col)| col.has_flag(flags::SEARCHABLE))
            .map(|(name, _)| self.dotted_path(name))
            .collect();
        if paths.is_empty() && self.search_unflagged_columns {
            return self
                .table
                .data_source()
                .search_table_condition(&self.table, text);
        }
        conditions::search_filter(&paths, text).into()
    }

    /// Dotted form of a spec column's BSON path, used for Mongo filters and
    /// sorts; a joined column's computed alias. Falls back to the spec name
    /// when the column isn't in the path map.
    fn dotted_path(&self, field: &str) -> String {
        if let Some(alias) = join_alias_of(&self.table, field) {
            return alias;
        }
        match self.column_paths.get(field) {
            Some(path) => path.join("."),
            None => field.to_string(),
        }
    }
}

/// Project a raw document onto the spec columns named in `column_paths`.
/// Identity (top-level keys → CBOR) when no paths are configured.
fn project_record(
    column_paths: &IndexMap<String, Vec<String>>,
    record: Record<AnyMongoType>,
) -> Record<CborValue> {
    if column_paths.is_empty() {
        return record
            .into_iter()
            .map(|(k, v)| (k, bson_to_cbor(v.value())))
            .collect();
    }
    let raw: IndexMap<String, Bson> = record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect();
    let mut out = Record::new();
    for (spec_name, path) in column_paths {
        if let Some(value) = walk_bson_path(&raw, path) {
            out.insert(spec_name.clone(), bson_to_cbor(&value));
        }
    }
    out
}

/// Walk a dotted BSON path against a top-level map. Returns `None` if any
/// intermediate segment is missing or isn't a `Document`.
fn walk_bson_path(map: &IndexMap<String, Bson>, path: &[String]) -> Option<Bson> {
    let head = path.first()?;
    let mut current = map.get(head)?.clone();
    for segment in &path[1..] {
        current = match current {
            Bson::Document(doc) => doc.get(segment)?.clone(),
            _ => return None,
        };
    }
    Some(current)
}

#[async_trait]
impl TableShell for MongoTableShell {
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
        self.read_all().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mongo_id = self.parse_id(id);
        let Some(record) = self.table.get_value(mongo_id.clone()).await? else {
            return Ok(None);
        };
        Ok(Some(self.unflatten_to_cbor(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((id, record)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((id.to_string(), self.unflatten_to_cbor(record))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.data_source().get_table_count(&self.table).await
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let mongo_record = self.flatten_for_write(record, None)?;
        let inserted = self
            .table
            .insert_value(mongo_id.clone(), &mongo_record)
            .await?;
        Ok(self.unflatten_to_cbor(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let stored = self.stored_row(&mongo_id).await?;
        let mongo_record = self.flatten_for_write(record, stored.as_ref())?;
        let replaced = self
            .table
            .replace_value(mongo_id.clone(), &mongo_record)
            .await?;
        Ok(self.unflatten_to_cbor(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let stored = self.stored_row(&mongo_id).await?;
        let mongo_partial = self.flatten_for_write(partial, stored.as_ref())?;
        let patched = self
            .table
            .patch_value(mongo_id.clone(), &mongo_partial)
            .await?;
        Ok(self.unflatten_to_cbor(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        let mongo_id = self.parse_id(id);
        self.table.delete(mongo_id.clone()).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let mongo_record = self.flatten_for_write(record, None)?;
        let id = self.table.insert_return_id_value(&mongo_record).await?;
        Ok(id.to_string())
    }

    /// All-or-nothing bulk insert in one transaction; see the `import` module.
    async fn import_vista_values(
        &self,
        _vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
    ) -> Result<usize> {
        if !self.capabilities.can_import {
            return Err(self.default_error("import_vista_values", "can_import"));
        }
        import::import(self, records).await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let dotted = self.dotted_path(field);
        self.table
            .add_condition(eq_filter(&dotted, &cbor_to_bson(value)));
        Ok(())
    }

    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for condition", field = field));
        }
        let filter = conditions::op_filter(&self.dotted_path(field), op, &cbor_to_bson(value))?;
        self.table.add_condition(filter);
        Ok(())
    }

    /// Push a boxed `bson::Document` or `MongoCondition` onto the wrapped
    /// table. Field names are raw BSON paths, not spec column names.
    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        self.table
            .add_condition(conditions::raw_condition(condition)?);
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // A reference carrying a Rhai `build_script` resolves through the
        // script engine instead of the foreign-key condition below.
        #[cfg(feature = "rhai")]
        if let Some(script) = self
            .metadata
            .references
            .get(relation)
            .and_then(|r| r.build_script.as_deref())
        {
            return self.get_ref_via_script(script, row);
        }

        let native_row: Record<AnyMongoType> = row
            .iter()
            .map(|(k, v)| (k.clone(), AnyMongoType::from(v.clone())))
            .collect();
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        self.factory().from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target_erased(relation)?;
        self.factory().from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        &self.metadata.contained
    }

    /// Resolve a contained relation. MongoDB stores nested objects/arrays
    /// natively, so the host value passes through unchanged and writes `$set`
    /// it back. The shared `Table::get_contained_ref` does the rest; this shim
    /// supplies the `MongoId` and the factory wrap.
    fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("_id");
        let parent_id = match row.get(id_field) {
            Some(CborValue::Text(s)) => s.clone(),
            Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
            _ => {
                return Err(error!(
                    "contained traversal requires the parent document's id",
                    relation = relation
                ));
            }
        };
        let mongo_id = self.parse_id(&parent_id);
        let factory = self.factory();
        self.table.get_contained_ref(
            relation,
            row,
            mongo_id,
            move |t| factory.from_table(t),
            |v| Some(v.clone()),
            |c| c,
        )
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        self.table.clear_orders();
        // MongoDB's `select_from_table` reads the field name from the first
        // key of the OrderBy expression's BSON doc; the value is ignored,
        // direction comes from the OrderBy::direction field.
        let dotted = self.dotted_path(field);
        let order = OrderBy {
            expression: MongoCondition::Doc(doc! { dotted: 1 }),
            direction: match dir {
                SortDirection::Ascending => TableSortDirection::Ascending,
                SortDirection::Descending => TableSortDirection::Descending,
            },
        };
        self.table.add_order(order);
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.table.clear_orders();
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        let condition = self.search_condition(text);
        self.current_search_handle = Some(self.table.temp_add_condition(condition));
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        Ok(())
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
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
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_page"))?;

        let mut page_table = self.table.clone();
        page_table.set_pagination(Some(Pagination::new(page as i64, size as i64)));
        let raw = page_table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect())
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let mut window_table = self.table.clone();
        window_table.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        let raw = window_table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect())
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_next"))?;

        // Keyset pagination: each page starts strictly after the previous
        // page's last row in the (total) sort order. The token belongs to the
        // sort it was issued under; see the `keyset` module for limits.
        let mut page_table = self.table.clone();
        keyset::add_id_tiebreak(&mut page_table);
        let sort = keyset::effective_sort(&page_table);
        if let Some(token) = token {
            let last = keyset::decode_token(&token)?;
            if last.len() != sort.len() {
                return Err(error!("invalid fetch_next token for mongodb driver"));
            }
            page_table.add_condition(keyset::seek_filter(&sort, &last));
        }
        page_table.set_pagination(Some(Pagination::new(1, size as i64)));
        let raw = page_table.list_values().await?;

        let next_token = match raw.last() {
            Some((_, last)) if raw.len() == size => Some(keyset::encode_token(
                &keyset::sort_values(&page_table, last, &sort),
            )?),
            _ => None,
        };
        let records = raw
            .into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect();
        Ok((records, next_token))
    }

    /// A change stream on the collection; see the `watch` module.
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        if !self.capabilities.can_subscribe {
            return Err(self.default_error("watch_vista", "can_subscribe"));
        }
        watch::watch(self).await
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "mongodb"
    }

    /// The read as it stands — collection, filter document, projection, sort
    /// and limit — rendered the way `MongoSelect` spells them: a `find`, or
    /// an `aggregate` over a pipeline view.
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        let source = self.table.data_source();
        let key = match source.view() {
            Some(_) => "aggregate",
            None => "find",
        };
        let collection = source.read_collection_name(self.table.table_name());
        let mut select = self.table.select();
        select.collection = Some(collection.to_string());
        // Both factory entry points resolve the joins before building the
        // vista, so a broken chain never gets this far.
        select.joins = table_joins(&self.table).unwrap_or_default();
        for field in &mut select.fields {
            if let Some(alias) = join_alias_of(&self.table, field) {
                *field = alias;
            }
        }
        let mut preview = serde_json::json!({
            "driver": "mongodb",
            "collection": collection,
        });
        preview[key] = select.preview().into();
        preview
    }

    /// BSON constructors (`oid`, `date`, `doc`) plus
    /// `self.with_condition(#{…})`, which pushes a filter document through
    /// [`add_raw_condition`](Self::add_raw_condition).
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        scripted::register_extensions(engine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::Document;

    #[test]
    fn walk_bson_path_returns_nested_value() {
        let mut inner = Document::new();
        inner.insert("city", "Hill Valley");
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::Document(inner));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, Some(Bson::String("Hill Valley".to_string())));
    }

    #[test]
    fn walk_bson_path_missing_segment_yields_none() {
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::Document(Document::new()));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, None);
    }

    #[test]
    fn walk_bson_path_through_scalar_yields_none() {
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::String("123 Main".into()));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, None);
    }
}
