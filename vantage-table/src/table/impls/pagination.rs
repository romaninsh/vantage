use vantage_types::Entity;

use crate::{pagination::Pagination, table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Set pagination configuration
    pub fn set_pagination(&mut self, pagination: Option<Pagination>) {
        self.pagination = pagination;
    }
}
