//! Vista bridge for the SQLite backend.
//!
//! Construct a `Vista` from a typed `Table<SqliteDB, E>` via
//! `SqliteDB::vista_factory()`, or from a YAML spec via
//! `SqliteVistaFactory::from_yaml`. The YAML path builds a
//! `Table<SqliteDB, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path.
//!
//! `AnySqliteType` already wraps `ciborium::Value`, so the boundary
//! translation is a passthrough; ids stringify (matching `TableSource::Id`).

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::SqliteVistaFactory;
pub use source::SqliteTableShell;
pub use spec::{
    SqliteColumnBlock, SqliteColumnExtras, SqliteTableBlock, SqliteTableExtras, SqliteVistaSpec,
};

use crate::sqlite::SqliteDB;

impl SqliteDB {
    /// Return a Vista factory bound to this SQLite data source.
    pub fn vista_factory(&self) -> SqliteVistaFactory {
        SqliteVistaFactory::new(self.clone())
    }
}
