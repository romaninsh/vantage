use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_expressions::AnyExpression;

use crate::{conditions::ConditionHandle, pagination::Pagination};

/// Dyn-safe trait for table operations.
#[async_trait]
pub trait TableLike: ReadableValueSet + WritableValueSet + Send + Sync {
    fn table_name(&self) -> &str;
    fn table_alias(&self) -> &str;

    /// Add a condition to this table using a type-erased expression
    /// The expression must be of type T::Expr for the underlying table's TableSource
    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()>;

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
}
