use std::collections::HashSet;

use crate::column::flags::ColumnFlag;

/// Trait defines a minimal implementation for a Table column
///
pub trait ColumnLike: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn alias(&self) -> Option<&str>;
    fn flags(&self) -> HashSet<ColumnFlag>;
    fn as_any(&self) -> &dyn std::any::Any;
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any>;
    fn get_type(&self) -> &'static str;
    fn clone_box(&self) -> Box<dyn ColumnLike>;
}
