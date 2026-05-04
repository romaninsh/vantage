//! Vista bridge for the MySQL backend.
//!
//! Construct a `Vista` from a typed `Table<MysqlDB, E>` via
//! `MysqlDB::vista_factory()`, or from a YAML spec via
//! `MysqlVistaFactory::from_yaml`. The YAML path builds a
//! `Table<MysqlDB, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path.
//!
//! `AnyMysqlType` already wraps `ciborium::Value`, so the boundary
//! translation is a passthrough; ids stringify (matching `TableSource::Id`).

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::MysqlVistaFactory;
pub use source::MysqlTableShell;
pub use spec::{
    MysqlColumnBlock, MysqlColumnExtras, MysqlTableBlock, MysqlTableExtras, MysqlVistaSpec,
};

use crate::mysql::MysqlDB;

impl MysqlDB {
    /// Return a Vista factory bound to this MySQL data source.
    pub fn vista_factory(&self) -> MysqlVistaFactory {
        MysqlVistaFactory::new(self.clone())
    }
}
