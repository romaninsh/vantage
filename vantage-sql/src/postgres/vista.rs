//! Vista bridge for the PostgreSQL backend: `PostgresDB::vista_factory()`
//! builds a `Vista` from a typed `Table<PostgresDB, E>` or, through
//! `VistaFactory::from_yaml`, from a YAML spec. The YAML path builds a
//! `Table<PostgresDB, EmptyEntity>` first and wraps it as `from_table` does.
//! Ids stringify, matching `TableSource::Id`.

pub mod factory;
#[cfg(feature = "rhai")]
pub mod rhai_source;
pub mod source;
pub mod spec;

pub use factory::PostgresVistaFactory;
pub use source::PostgresTableShell;
pub use spec::{
    InheritBlock, PostgresColumnBlock, PostgresColumnExtras, PostgresTableBlock,
    PostgresTableExtras, PostgresVistaSpec,
};

use crate::postgres::PostgresDB;

impl PostgresDB {
    /// Return a Vista factory bound to this PostgreSQL data source.
    pub fn vista_factory(&self) -> PostgresVistaFactory {
        PostgresVistaFactory::new(self.clone())
    }
}
