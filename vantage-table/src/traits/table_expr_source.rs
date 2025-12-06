use serde_json::Value;
use vantage_core::Result;
use vantage_expressions::{
    ExprDataSource, Expression, SelectableDataSource, traits::datasource::DataSource,
};
use vantage_types::Entity;

use crate::{prelude::TableSource, table::Table};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
pub trait TableExprSource<Ex = Expression<Value>>:
    DataSource + TableSource + ExprDataSource<Ex>
{
    /// Get a select query for all data from a table (for ReadableValueSet implementation)
    fn get_table_expr_count<E: Entity<Self::Value>>(
        &self,
        table: &Table<Self, E>,
    ) -> Expression<Ex>;

    // /// Get an insert query for a record into a table (for InsertableValueSet implementation)
    // fn get_table_insert_query<E: Entity<Self::Value>>(
    //     &self,
    //     table: &Table<Self, E>,
    //     record: &vantage_types::Record<Self::Value>,
    // ) -> Result<Self::Insert>;
}
