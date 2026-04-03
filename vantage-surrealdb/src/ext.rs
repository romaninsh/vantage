//! SurrealDB-specific extension methods for `Table<SurrealDB, E>`.

use vantage_core::Result;
use vantage_expressions::result;
use vantage_table::table::Table;
use vantage_types::Entity;

use crate::select::SurrealSelect;
use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;

/// Extension trait providing SurrealDB-specific query builders on `Table<SurrealDB, E>`.
///
/// These narrow the generic `SurrealSelect<Rows>` returned by `table.select()`
/// into more specific result types that SurrealDB supports natively.
pub trait SurrealTableExt<E: Entity<AnySurrealType>> {
    /// `SELECT col1, col2, … FROM ONLY table [WHERE …]` — first row only.
    fn select_first(&self) -> SurrealSelect<result::SingleRow>;

    /// `SELECT VALUE <col> FROM table [WHERE …]` — single column, all rows.
    fn select_column(&self, column: &str) -> Result<SurrealSelect<result::List>>;

    /// `SELECT VALUE <col> FROM ONLY table [WHERE …]` — single column, first row.
    fn select_single(&self, column: &str) -> Result<SurrealSelect<result::Single>>;
}

impl<E: Entity<AnySurrealType>> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn select_first(&self) -> SurrealSelect<result::SingleRow> {
        self.select().only_first_row()
    }

    fn select_column(&self, column: &str) -> Result<SurrealSelect<result::List>> {
        if !self.columns().contains_key(column) {
            return Err(vantage_core::error!(
                "Column not found in table",
                column = column,
                table = self.table_name()
            ));
        }
        Ok(self.select().only(column))
    }

    fn select_single(&self, column: &str) -> Result<SurrealSelect<result::Single>> {
        if !self.columns().contains_key(column) {
            return Err(vantage_core::error!(
                "Column not found in table",
                column = column,
                table = self.table_name()
            ));
        }
        Ok(self.select().only(column).only_first_row())
    }
}
