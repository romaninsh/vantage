//! Type-erased table wrapper with downcasting support
//!
//! `AnyTable` provides a way to store tables of different types uniformly
//! while preserving the ability to recover the concrete type through downcasting.
//!
//! Tables whose value/id types differ from `ciborium::Value`/`String` can still
//! be wrapped via [`AnyTable::from_table`] as long as the value type implements
//! `Into<CborValue> + From<CborValue>` and the id type implements
//! `Display + From<String>`.
//!
//! Each backend brings its own native value type (`AnySurrealType`,
//! `AnyMongoType`, `AnyPostgresType`, etc.); CBOR is just the carrier across
//! the type-erased boundary, chosen because it preserves type fidelity that
//! JSON loses (integer vs float, binary blobs, NaN/Infinity, precise
//! decimals).

use std::any::TypeId;
use std::fmt::Display;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, ValueSet, WritableValueSet};
use vantage_types::{Entity, Record};

// Type alias for cleaner code
pub type AnyRecord = Record<CborValue>;

use crate::{
    conditions::ConditionHandle,
    pagination::Pagination,
    table::Table,
    traits::{table_like::TableLike, table_source::TableSource},
};

/// Type-erased table that can be downcast to concrete `Table<T, E>`
/// Works with AnyRecord (which uses ciborium::Value)
pub struct AnyTable {
    inner: Box<dyn TableLike<Value = CborValue, Id = String>>,
    datasource_type_id: TypeId,
    entity_type_id: TypeId,
    datasource_name: &'static str,
    entity_name: &'static str,
}

impl AnyTable {
    /// Create a new AnyTable from a concrete table.
    /// Only works with tables that use `ciborium::Value` as their Value type.
    pub fn new<
        T: TableSource<Value = CborValue, Id = String> + 'static,
        E: Entity<CborValue> + 'static,
    >(
        table: Table<T, E>,
    ) -> Self {
        Self {
            inner: Box::new(table),
            datasource_type_id: TypeId::of::<T>(),
            entity_type_id: TypeId::of::<E>(),
            datasource_name: std::any::type_name::<T>(),
            entity_name: std::any::type_name::<E>(),
        }
    }

    /// Create an AnyTable from a table with any value/id types that convert
    /// to/from CBOR.
    ///
    /// This wraps the table in an internal adapter that converts values on the
    /// fly, so any `TableSource` can be used with the unified `AnyTable`
    /// interface.
    pub fn from_table<T, E>(table: Table<T, E>) -> Self
    where
        T: TableSource + 'static,
        T::Value: Into<CborValue> + From<CborValue>,
        T::Id: Display + From<String>,
        E: Entity<T::Value> + 'static,
    {
        Self {
            inner: Box::new(CborAdapter { inner: table }),
            datasource_type_id: TypeId::of::<T>(),
            entity_type_id: TypeId::of::<E>(),
            datasource_name: std::any::type_name::<T>(),
            entity_name: std::any::type_name::<E>(),
        }
    }

    /// Wrap any `TableLike<Value = CborValue, Id = String>` directly. Use
    /// this for table-like wrappers (e.g. `vantage_live::LiveTable`) that
    /// aren't a `Table<T, E>` instance themselves but already satisfy the
    /// `AnyTable`-facing trait surface. The entity type is recorded as
    /// `()` since wrappers don't carry one.
    pub fn from_table_like<T>(table: T) -> Self
    where
        T: TableLike<Value = CborValue, Id = String> + 'static,
    {
        Self {
            inner: Box::new(table),
            datasource_type_id: TypeId::of::<T>(),
            entity_type_id: TypeId::of::<()>(),
            datasource_name: std::any::type_name::<T>(),
            entity_name: "<wrapped>",
        }
    }

    /// Attempt to downcast to a concrete `Table<T, E>`
    ///
    /// Returns `Err(self)` if the type doesn't match, allowing recovery
    pub fn downcast<
        T: TableSource<Value = CborValue, Id = String> + 'static,
        E: Entity<CborValue> + 'static,
    >(
        self,
    ) -> Result<Table<T, E>> {
        // Check TypeIds for better error messages
        if self.datasource_type_id != TypeId::of::<T>() {
            let expected = std::any::type_name::<T>();
            return Err(error!(
                "DataSource type mismatch",
                expected = expected,
                actual = self.datasource_name
            ));
        }
        if self.entity_type_id != TypeId::of::<E>() {
            let expected = std::any::type_name::<E>();
            return Err(error!(
                "Entity type mismatch",
                expected = expected,
                actual = self.entity_name
            ));
        }

        // Perform the actual downcast
        self.inner
            .into_any()
            .downcast::<Table<T, E>>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Failed to downcast table"))
    }

    /// Get the datasource type name for debugging
    pub fn datasource_name(&self) -> &str {
        self.datasource_name
    }

    /// Get the entity type name for debugging
    pub fn entity_name(&self) -> &str {
        self.entity_name
    }

    /// Get the datasource TypeId
    pub fn datasource_type_id(&self) -> TypeId {
        self.datasource_type_id
    }

    /// Get the entity TypeId
    pub fn entity_type_id(&self) -> TypeId {
        self.entity_type_id
    }

    /// Check if this table matches the given types
    pub fn is_type<T: TableSource + 'static, E: Entity<CborValue> + 'static>(&self) -> bool {
        self.datasource_type_id == TypeId::of::<T>() && self.entity_type_id == TypeId::of::<E>()
    }
}

impl Clone for AnyTable {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
            datasource_type_id: self.datasource_type_id,
            entity_type_id: self.entity_type_id,
            datasource_name: self.datasource_name,
            entity_name: self.entity_name,
        }
    }
}

impl std::fmt::Debug for AnyTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyTable")
            .field("datasource", &self.datasource_name)
            .field("entity", &self.entity_name)
            .finish()
    }
}

// Implement ValueSet first
impl ValueSet for AnyTable {
    type Id = String;
    type Value = CborValue;
}

// Implement ReadableValueSet by delegating to inner TableLike
#[async_trait]
impl ReadableValueSet for AnyTable {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        self.inner.list_values().await
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Option<Record<Self::Value>>> {
        self.inner.get_value(id).await
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        self.inner.get_some_value().await
    }
}

// Implement WritableValueSet by delegating to inner TableLike
#[async_trait]
impl WritableValueSet for AnyTable {
    async fn insert_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.inner.insert_value(id, record).await
    }

    async fn replace_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.inner.replace_value(id, record).await
    }

    async fn patch_value(
        &self,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.inner.patch_value(id, partial).await
    }

    async fn delete(&self, id: &Self::Id) -> Result<()> {
        self.inner.delete(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.inner.delete_all().await
    }
}

// Implement TableLike by delegating to inner
#[async_trait]
impl TableLike for AnyTable {
    fn table_name(&self) -> &str {
        self.inner.table_name()
    }

    fn table_alias(&self) -> &str {
        self.inner.table_alias()
    }

    fn column_names(&self) -> Vec<String> {
        self.inner.column_names()
    }

    fn id_field_name(&self) -> Option<String> {
        self.inner.id_field_name()
    }

    fn title_field_names(&self) -> Vec<String> {
        self.inner.title_field_names()
    }

    fn column_types(&self) -> IndexMap<String, &'static str> {
        self.inner.column_types()
    }

    fn get_ref_names(&self) -> Vec<String> {
        self.inner.get_ref_names()
    }

    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        self.inner.add_condition(condition)
    }

    fn add_condition_eq(&mut self, field: &str, value: &str) -> Result<()> {
        self.inner.add_condition_eq(field, value)
    }

    fn temp_add_condition(
        &mut self,
        condition: vantage_expressions::AnyExpression,
    ) -> Result<ConditionHandle> {
        self.inner.temp_add_condition(condition)
    }

    fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        self.inner.temp_remove_condition(handle)
    }

    fn search_expression(&self, search_value: &str) -> Result<vantage_expressions::AnyExpression> {
        self.inner.search_expression(search_value)
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
        self.inner.set_pagination(pagination)
    }

    fn get_pagination(&self) -> Option<&Pagination> {
        self.inner.get_pagination()
    }

    async fn get_count(&self) -> vantage_core::Result<i64> {
        self.inner.get_count().await
    }

    fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        self.inner.get_ref(relation)
    }
}

impl AnyTable {
    /// Traverse a named reference and return the related table as `AnyTable`.
    ///
    /// Inherent wrapper so callers don't need `use TableLike` in scope.
    pub fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        TableLike::get_ref(self, relation)
    }

    /// Configure pagination using a callback
    pub fn with_pagination<F>(&mut self, func: F)
    where
        F: FnOnce(&mut Pagination),
    {
        let mut pagination = self.inner.get_pagination().copied().unwrap_or_default();
        func(&mut pagination);
        self.inner.set_pagination(Some(pagination));
    }
}

// ── CborAdapter: blanket bridge for non-CBOR table types ────────────────

/// Wraps a `Table<T, E>` whose value/id types are not `ciborium::Value`/`String`,
/// converting on the fly so it can satisfy `TableLike<Value = CborValue, Id = String>`.
struct CborAdapter<T: TableSource, E: Entity<T::Value>> {
    inner: Table<T, E>,
}

impl<T: TableSource, E: Entity<T::Value>> Clone for CborAdapter<T, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T, E> CborAdapter<T, E>
where
    T: TableSource,
    T::Value: Into<CborValue>,
    T::Id: Display,
    E: Entity<T::Value>,
{
    fn convert_record(record: Record<T::Value>) -> Record<CborValue> {
        record.into_iter().map(|(k, v)| (k, v.into())).collect()
    }

    fn convert_record_back(record: &Record<CborValue>) -> Record<T::Value>
    where
        T::Value: From<CborValue>,
    {
        record
            .iter()
            .map(|(k, v)| (k.clone(), T::Value::from(v.clone())))
            .collect()
    }
}

impl<T, E> ValueSet for CborAdapter<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    type Id = String;
    type Value = CborValue;
}

#[async_trait]
impl<T, E> ReadableValueSet for CborAdapter<T, E>
where
    T: TableSource,
    T::Value: Into<CborValue>,
    T::Id: Display + From<String>,
    E: Entity<T::Value>,
{
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self
            .inner
            .data_source()
            .list_table_values(&self.inner)
            .await?;
        Ok(raw
            .into_iter()
            .map(|(id, rec)| (id.to_string(), Self::convert_record(rec)))
            .collect())
    }

    async fn get_value(&self, id: &String) -> Result<Option<Record<CborValue>>> {
        let native_id: T::Id = id.clone().into();
        let rec = self
            .inner
            .data_source()
            .get_table_value(&self.inner, &native_id)
            .await?;
        Ok(rec.map(Self::convert_record))
    }

    async fn get_some_value(&self) -> Result<Option<(String, Record<CborValue>)>> {
        let result = self
            .inner
            .data_source()
            .get_table_some_value(&self.inner)
            .await?;
        Ok(result.map(|(id, rec)| (id.to_string(), Self::convert_record(rec))))
    }
}

#[async_trait]
impl<T, E> WritableValueSet for CborAdapter<T, E>
where
    T: TableSource,
    T::Value: Into<CborValue> + From<CborValue>,
    T::Id: Display + From<String>,
    E: Entity<T::Value>,
{
    async fn insert_value(
        &self,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let native_id: T::Id = id.clone().into();
        let native_rec = Self::convert_record_back(record);
        let returned = self
            .inner
            .data_source()
            .insert_table_value(&self.inner, &native_id, &native_rec)
            .await?;
        Ok(Self::convert_record(returned))
    }

    async fn replace_value(
        &self,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let native_id: T::Id = id.clone().into();
        let native_rec = Self::convert_record_back(record);
        let returned = self
            .inner
            .data_source()
            .replace_table_value(&self.inner, &native_id, &native_rec)
            .await?;
        Ok(Self::convert_record(returned))
    }

    async fn patch_value(
        &self,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let native_id: T::Id = id.clone().into();
        let native_rec = Self::convert_record_back(partial);
        let returned = self
            .inner
            .data_source()
            .patch_table_value(&self.inner, &native_id, &native_rec)
            .await?;
        Ok(Self::convert_record(returned))
    }

    async fn delete(&self, id: &String) -> Result<()> {
        let native_id: T::Id = id.clone().into();
        self.inner
            .data_source()
            .delete_table_value(&self.inner, &native_id)
            .await
    }

    async fn delete_all(&self) -> Result<()> {
        self.inner
            .data_source()
            .delete_table_all_values(&self.inner)
            .await
    }
}

#[async_trait]
impl<T, E> TableLike for CborAdapter<T, E>
where
    T: TableSource + 'static,
    T::Value: Into<CborValue> + From<CborValue>,
    T::Id: Display + From<String>,
    E: Entity<T::Value> + 'static,
{
    fn table_name(&self) -> &str {
        self.inner.table_name()
    }

    fn table_alias(&self) -> &str {
        self.inner.table_name()
    }

    fn column_names(&self) -> Vec<String> {
        self.inner.columns().keys().cloned().collect()
    }

    fn id_field_name(&self) -> Option<String> {
        TableLike::id_field_name(&self.inner)
    }

    fn title_field_names(&self) -> Vec<String> {
        TableLike::title_field_names(&self.inner)
    }

    fn column_types(&self) -> IndexMap<String, &'static str> {
        TableLike::column_types(&self.inner)
    }

    fn get_ref_names(&self) -> Vec<String> {
        TableLike::get_ref_names(&self.inner)
    }

    fn add_condition(&mut self, _condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        Err(error!("add_condition not supported through CborAdapter"))
    }

    fn add_condition_eq(&mut self, field: &str, value: &str) -> Result<()> {
        TableLike::add_condition_eq(&mut self.inner, field, value)
    }

    fn temp_add_condition(
        &mut self,
        _condition: vantage_expressions::AnyExpression,
    ) -> Result<ConditionHandle> {
        Err(error!(
            "temp_add_condition not supported through CborAdapter"
        ))
    }

    fn temp_remove_condition(&mut self, _handle: ConditionHandle) -> Result<()> {
        Err(error!(
            "temp_remove_condition not supported through CborAdapter"
        ))
    }

    fn search_expression(&self, _search_value: &str) -> Result<vantage_expressions::AnyExpression> {
        Err(error!(
            "search_expression not supported through CborAdapter"
        ))
    }

    fn clone_box(&self) -> Box<dyn TableLike<Value = CborValue, Id = String>> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn as_any_ref(&self) -> &dyn std::any::Any {
        self
    }

    fn set_pagination(&mut self, pagination: Option<Pagination>) {
        self.inner.set_pagination(pagination);
    }

    fn get_pagination(&self) -> Option<&Pagination> {
        self.inner.pagination()
    }

    async fn get_count(&self) -> Result<i64> {
        self.inner.data_source().get_table_count(&self.inner).await
    }

    fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        self.inner.get_ref(relation)
    }
}

// Inline AnyTable tests using `MockTableSource` were dropped as part of the
// CBOR swap. `MockTableSource` still uses `serde_json::Value`, but `AnyTable`
// now requires `Value = CborValue`. A new CBOR-flavoured mock + restored
// integration tests will live in `tests/table_like.rs` once `MockTableSource`
// (or a sibling mock) gains CBOR support — see TODO.md "Architecture: Make
// ImTable / ImDataSource generic over Value".
