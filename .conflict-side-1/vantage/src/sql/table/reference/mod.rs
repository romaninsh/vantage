pub mod many;
pub mod one;

use super::SqlTable;
use std::fmt::Debug;

pub type RelatedTableFx = dyn Fn() -> Box<dyn SqlTable> + Send + Sync + 'static;

pub trait RelatedSqlTable: Debug + Send + Sync {
    fn get_related_set(&self, _table: &dyn SqlTable) -> Box<dyn SqlTable>;
    fn get_linked_set(&self, _table: &dyn SqlTable) -> Box<dyn SqlTable>;
}
