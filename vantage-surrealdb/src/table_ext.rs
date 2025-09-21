use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_table::{Entity, Table};

use crate::{
    SurrealDB, associated_query::SurrealAssociated, select::SurrealSelect,
    surreal_return::SurrealReturn,
};
use vantage_expressions::protocol::result;

/// Extension trait for Table<SurrealDB, E> providing SurrealDB-specific query methods
#[async_trait::async_trait]
pub trait SurrealTableExt<E: Entity> {
    /// Create a SurrealAssociated that returns multiple rows with all columns
    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>>;

    /// Create a SurrealAssociated that returns the first row with all columns
    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E>;

    /// Create a SurrealAssociated that returns a single column from all rows
    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>, String>;

    /// Create a SurrealAssociated that returns a single value (first row, single column)
    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>, String>;

    /// Execute a select query and return the results directly
    async fn surreal_get(&self) -> vantage_expressions::util::error::Result<Vec<E>>;

    /// Create a count query that returns the number of rows
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64>;
}

#[async_trait::async_trait]
impl<E: Entity> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>> {
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

        SurrealAssociated::new(select, self.data_source().clone())
    }

    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E> {
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

        let single_row_select = select.only_first_row();
        SurrealAssociated::new(single_row_select, self.data_source().clone())
    }

    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>, String>
    {
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

        Ok(SurrealAssociated::new(
            list_select,
            self.data_source().clone(),
        ))
    }

    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>, String> {
        let column_name = column.into();

        // Validate column exists
        if !self.columns().contains_key(&column_name) {
            return Err(format!("Column '{}' not found in table", column_name));
        }

        let column_obj = &self.columns()[&column_name];
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        let single_select = select.only_first_row().only_column(column_obj.name());
        Ok(SurrealAssociated::new(
            single_select,
            self.data_source().clone(),
        ))
    }

    async fn surreal_get(&self) -> vantage_expressions::util::error::Result<Vec<E>> {
        use vantage_expressions::AssociatedQueryable;
        self.select_surreal().get().await
    }

    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64> {
        let count_return = self.select_surreal().query.as_count();
        SurrealAssociated::new(count_return, self.data_source().clone())
    }
}
