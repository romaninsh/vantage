use serde_json::Value;
use std::collections::HashSet;

use crate::column::{core::ColumnType, flags::ColumnFlag};

/// Trait defines a minimal implementation for a Table column with type information
///
pub trait ColumnLike<T = Value>: Send + Sync + std::fmt::Debug
where
    T: ColumnType,
{
    fn name(&self) -> &str;
    fn alias(&self) -> Option<&str> {
        None
    }
    fn flags(&self) -> HashSet<ColumnFlag>;
    fn as_any(&self) -> &dyn std::any::Any;
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any>;
    fn get_type(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}
