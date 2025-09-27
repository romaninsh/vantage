use crate::SurrealDB;

impl vantage_table::TableSource for SurrealDB {
    type Column = crate::SurrealColumn;

    fn create_column(&self, name: &str, _table: impl vantage_table::TableLike) -> Self::Column {
        crate::SurrealColumn::new(name)
    }
}
