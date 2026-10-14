use vantage_types::Entity;

use crate::{
    column::flags::ColumnFlag,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// The columns a quicksearch reads: those flagged
    /// [`Searchable`](ColumnFlag::Searchable), or, when none is, every
    /// column. Dotted paths and imported columns are the driver's to
    /// exclude. `VistaMetadata::search_columns` states the same rule over a
    /// vista's columns.
    pub fn search_columns(&self) -> Vec<&T::Column<T::AnyType>> {
        let flagged: Vec<_> = self
            .columns()
            .values()
            .filter(|column| column.flags().contains(&ColumnFlag::Searchable))
            .collect();
        if flagged.is_empty() {
            self.columns().values().collect()
        } else {
            flagged
        }
    }
}

#[cfg(test)]
mod tests {
    use vantage_types::EmptyEntity;

    use crate::mocks::{mock_column::MockColumn, mock_table_source::MockTableSource};

    use super::*;

    fn names<T: TableSource, E: Entity<T::Value>>(table: &Table<T, E>) -> Vec<String> {
        table
            .search_columns()
            .into_iter()
            .map(|column| column.name().to_string())
            .collect()
    }

    #[test]
    fn search_columns_are_the_flagged_ones_or_every_column() {
        let mut table = Table::<_, EmptyEntity>::new("t", MockTableSource::new())
            .with_column(MockColumn::<String>::new("name"))
            .with_column(MockColumn::<String>::new("note"));
        assert_eq!(names(&table), ["name", "note"]);

        table.add_column(MockColumn::<String>::new("title").with_flag(ColumnFlag::Searchable));
        assert_eq!(names(&table), ["title"]);
    }
}
