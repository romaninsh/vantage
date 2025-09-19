use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_table::{Entity, Table};

use crate::{SurrealDB, select::SurrealSelect};
use vantage_expressions::protocol::result;

/// Extension trait for Table<SurrealDB, E> providing SurrealDB-specific query methods
pub trait SurrealTableExt<E: Entity> {
    /// Create a SurrealSelect query that returns multiple rows with all columns
    fn select_surreal(&self) -> SurrealSelect<result::Rows>;

    /// Create a SurrealSelect query that returns the first row with all columns
    fn select_surreal_first(&self) -> SurrealSelect<result::SingleRow>;

    /// Create a SurrealSelect query that returns a single column from all rows
    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealSelect<result::List>, String>;

    /// Create a SurrealSelect query that returns a single value (first row, single column)
    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealSelect<result::Single>, String>;
}

impl<E: Entity> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn select_surreal(&self) -> SurrealSelect<result::Rows> {
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(expr!(column.name()), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        select
    }

    fn select_surreal_first(&self) -> SurrealSelect<result::SingleRow> {
        self.select_surreal().only_first_row()
    }

    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealSelect<result::List>, String> {
        let column_name = column.into();

        // Validate column exists
        if !self.columns().contains_key(&column_name) {
            return Err(format!("Column '{}' not found in table", column_name));
        }

        let column_obj = &self.columns()[&column_name];
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        // Add only the requested column
        let mut list_select = select.only_column(column_obj.name());

        for condition in self.conditions() {
            list_select.add_where_condition(condition.clone());
        }

        Ok(list_select)
    }

    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealSelect<result::Single>, String> {
        let column_name = column.into();

        // Validate column exists
        if !self.columns().contains_key(&column_name) {
            return Err(format!("Column '{}' not found in table", column_name));
        }

        let column_obj = &self.columns()[&column_name];
        let single_select = self.select_surreal_first().only_column(column_obj.name());

        Ok(single_select)
    }
}
