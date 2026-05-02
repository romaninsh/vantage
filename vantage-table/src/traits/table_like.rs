use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_expressions::AnyExpression;

use crate::{any::AnyTable, conditions::ConditionHandle, pagination::Pagination};

/// Dyn-safe trait for table operations.
#[async_trait]
pub trait TableLike: ReadableValueSet + WritableValueSet + Send + Sync {
    fn table_name(&self) -> &str;
    fn table_alias(&self) -> &str;
    fn column_names(&self) -> Vec<String>;

    /// Name of the column flagged as the id field, if any.
    fn id_field_name(&self) -> Option<String> {
        None
    }

    /// Names of columns flagged as `TitleField`.
    fn title_field_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Map of column name -> original Rust type name. Backends that
    /// preserve type metadata (e.g. `Column::get_type()`) override this
    /// so generic UIs can drive type-aware rendering without poking at
    /// concrete column types.
    fn column_types(&self) -> IndexMap<String, &'static str> {
        IndexMap::new()
    }

    /// Names of relations traversable via `get_ref`.
    fn get_ref_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Add a condition to this table using a type-erased expression
    /// The expression must be of type T::Expr for the underlying table's TableSource
    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()>;

    /// Add a permanent equality condition expressed as raw strings.
    ///
    /// Generic CLIs (and other type-erased callers) work with
    /// `field=value` text and cannot reach into `T::Condition`. Each
    /// backend that supports textual eq filtering overrides this; the
    /// default returns an error.
    fn add_condition_eq(&mut self, field: &str, value: &str) -> Result<()> {
        let _ = (field, value);
        Err(vantage_core::error!(
            "add_condition_eq not supported on this TableLike"
        ))
    }

    /// Add a temporary condition using AnyExpression that can be removed later
    fn temp_add_condition(&mut self, condition: AnyExpression) -> Result<ConditionHandle>;

    /// Remove a temporary condition by its handle
    fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()>;

    /// Create a search expression for this table
    fn search_expression(&self, search_value: &str) -> Result<AnyExpression>;

    /// Clone into a Box for object-safe cloning
    fn clone_box(&self) -> Box<dyn TableLike<Value = Self::Value, Id = Self::Id>>;

    /// Convert to Any for downcasting
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any>;
    fn as_any_ref(&self) -> &dyn std::any::Any;

    /// Set pagination for this table
    fn set_pagination(&mut self, pagination: Option<Pagination>);

    /// Get pagination for this table
    fn get_pagination(&self) -> Option<&Pagination>;

    /// Get count of records in the table
    async fn get_count(&self) -> Result<i64>;

    /// Traverse a named reference and return the related table as `AnyTable`.
    ///
    /// Default impl returns an error so wrappers without ref support compile
    /// unchanged. `Table<T, E>` overrides this to delegate to its inherent
    /// `get_ref`; `AnyTable`, `CborAdapter` and `LiveTable` override to forward
    /// through to the underlying table that holds the refs.
    fn get_ref(&self, _relation: &str) -> Result<AnyTable> {
        Err(vantage_core::error!(
            "get_ref not supported on this TableLike"
        ))
    }
}
