use vantage_dataset::{DataSet, ValueSet};
use vantage_types::Entity;

use crate::{prelude::TableSource, table::Table};

pub mod insertable_dataset;
pub mod insertable_value_set;
pub mod readable_dataset;
pub mod readable_value_set;
pub mod writable_dataset;
pub mod writable_value_set;

impl<T: TableSource, E: Entity<T::Value>> ValueSet for Table<T, E> {
    type Id = T::Id;
    type Value = T::Value;
}

impl<T: TableSource, E: Entity<T::Value>> DataSet<E> for Table<T, E> {}
