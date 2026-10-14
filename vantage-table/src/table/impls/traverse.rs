//! The shared `get_ref` body of drivers whose relations are typed
//! `with_one` / `with_many` joins.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{Reference, Vista};

use crate::{table::Table, traits::table_source::TableSource};

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// Traverse `reference`, a relation from this table's Vista metadata.
    /// The bare target is built from the typed relation of the same name,
    /// turned into a `Vista` by `wrap` (the driver's factory), then narrowed
    /// by the reference's join keys when a parent `row` is given.
    pub fn traverse_erased(
        &self,
        reference: &Reference,
        row: Option<&Record<CborValue>>,
        wrap: impl FnOnce(Table<T, EmptyEntity>) -> Result<Vista>,
    ) -> Result<Vista> {
        let target = wrap(self.get_ref_target_erased(&reference.name)?)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }
}
