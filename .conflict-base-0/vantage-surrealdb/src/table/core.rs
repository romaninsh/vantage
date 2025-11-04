//! # SurrealDB Table Core Operations
//!
//! This module provides the core Extensions that Table<SurrealDB> would have.
//! You only need to include prelude::* to use this

use vantage_core::{Result, vantage_error};
use vantage_expressions::result;
use vantage_table::{ColumnLike, Entity, Table};

use crate::operation::RefOperation;
use crate::{SurrealAssociated, SurrealColumn, SurrealDB, SurrealSelect};

/// Core trait for SurrealDB table operations that other traits can build upon
pub trait SurrealTableCore<E: Entity> {
    /// Add ID condition using String ID
    fn with_id(self, id: &str) -> Self;

    /// Create a SurrealAssociated that returns multiple rows with all columns
    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>>;

    /// Create a SurrealAssociated that returns the first row with all columns
    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E>;

    /// Create a SurrealAssociated that returns a single column from all rows
    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>>;

    /// Create a SurrealAssociated that returns a single value (first row, single column)
    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>>;

    // Overrides Queryable on a table
    // fn select(&self) -> SurrealSelect<result::Rows>;

    // /// Execute a select query and return the results directly
    // async fn surreal_get(&self) -> Result<Vec<E>>;
}

impl<E: Entity> SurrealTableCore<E> for Table<SurrealDB, E> {
    fn with_id(self, id: &str) -> Self {
        let id_col = SurrealColumn::<String>::new("id");
        self.with_condition(id_col.eq(id))
    }

    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>> {
        SurrealAssociated::new(self.select(), self.data_source().clone())
        // let mut select = SurrealSelect::new();

        // select.set_source(self.table_name(), None);

        // for column in self.columns().values() {
        //     match column.alias() {
        //         Some(alias) => select.add_expression(expr!(column.name()), Some(alias.to_string())),
        //         None => select.add_field(column.name()),
        //     }
        // }

        // for condition in self.conditions() {
        //     select.add_where_condition(condition.clone());
        // }

        // select
    }

    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E> {
        SurrealAssociated::new(self.select().only_first_row(), self.data_source().clone())
    }

    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>> {
        let column = column.into();
        let Some(col) = self.columns().get(&column) else {
            return Err(vantage_error!("Column '{}' not found in table", &column));
        };

        // Add only the requested column
        let select = self.select().only_column(col.name());

        Ok(SurrealAssociated::new(select, self.data_source().clone()))
    }

    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>> {
        let column = column.into();
        let Some(col) = self.columns().get(&column) else {
            return Err(vantage_error!("Column '{}' not found in table", &column));
        };

        // Add only the requested column
        let select = self.select().only_column(col.name()).only_first_row();

        Ok(SurrealAssociated::new(select, self.data_source().clone()))
    }
}

#[cfg(test)]
mod tests {
    use crate::mocks::SurrealMockBuilder;

    use super::*;

    #[test]
    fn test_with_id_api() {
        let t = Table::new("t", SurrealMockBuilder::new().build());

        assert_eq!(
            t.with_id("abc").select().preview(),
            "SELECT * FROM t WHERE id = \"abc\""
        );
    }
}
