//! Vista bridge for SpacetimeDB.
//!
//! Build a `Vista` from a table or view name via [`SpacetimeDb::vista_factory`],
//! which reads the module schema and derives the columns, the id column and the
//! capabilities from it — SpacetimeDB is fully self-describing, so nothing has
//! to be declared twice.

pub mod factory;
pub mod source;

pub use factory::SpacetimeVistaFactory;
pub use source::SpacetimeTableShell;

use crate::client::SpacetimeDb;

impl SpacetimeDb {
    /// A Vista factory bound to this database.
    pub fn vista_factory(&self) -> SpacetimeVistaFactory {
        SpacetimeVistaFactory::new(self.clone())
    }
}
