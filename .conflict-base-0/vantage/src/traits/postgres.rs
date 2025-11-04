use crate::table::Table;
use crate::Field;

pub trait PostgresTableDataSet<T> {
    fn table(&self) -> &Table;

    fn id(&self) -> &Field {
        self.table().id()
    }

    fn map<F>(&self, f: F) -> T
    where
        F: Fn(&T) -> T;
}
