use crate::TableLike;
use vantage_expressions::Expression;

/// Trait for table data sources that defines column type separate from execution
pub trait TableSource: Send + Sync {
    type Column: ColumnLike + Clone + 'static;

    /// Create a new column with the given name
    fn create_column(&self, name: &str, table: impl TableLike) -> Self::Column;
}

/// Minimal trait for column-like objects
pub trait ColumnLike: Send + Sync {
    fn name(&self) -> &str;
    fn alias(&self) -> Option<&str>;
    fn expr(&self) -> Expression;
}
