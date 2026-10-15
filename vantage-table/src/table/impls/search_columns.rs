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
