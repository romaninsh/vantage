//! Vista bridge for the PostgreSQL backend.
//!
//! Construct a `Vista` from a typed `Table<PostgresDB, E>` via
//! `PostgresDB::vista_factory()`, or from a YAML spec via
//! `PostgresVistaFactory::from_yaml`. The YAML path builds a
//! `Table<PostgresDB, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path.
//!
//! `AnyPostgresType` already wraps `ciborium::Value`, so the boundary
//! translation is a passthrough; ids stringify (matching `TableSource::Id`).

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::PostgresVistaFactory;
pub use source::PostgresTableShell;
pub use spec::{
    PostgresColumnBlock, PostgresColumnExtras, PostgresTableBlock, PostgresTableExtras,
    PostgresVistaSpec,
};

use crate::postgres::PostgresDB;

impl PostgresDB {
    /// Return a Vista factory bound to this PostgreSQL data source.
    pub fn vista_factory(&self) -> PostgresVistaFactory {
        PostgresVistaFactory::new(self.clone())
    }
}
