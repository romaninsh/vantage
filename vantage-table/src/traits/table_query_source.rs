use serde_json::Value;
use vantage_core::Result;
use vantage_expressions::{SelectableDataSource, traits::datasource::DataSource};
use vantage_types::Entity;

use crate::{prelude::TableSource, table::Table};

/// A table source that can express a table read as a select query.
pub trait TableQuerySource<T = Value>: DataSource + TableSource + SelectableDataSource<T>
where
    T: Clone,
{
    /// The select query that reads every row of `table`.
    fn get_table_select_query<E: Entity<Self::Value>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Self::Select>;
}
