//! Vista bridge for the SQLite backend: `SqliteDB::vista_factory()` builds a
//! `Vista` from a typed `Table<SqliteDB, E>` or, through
//! `VistaFactory::from_yaml`, from a YAML spec. The YAML path builds a
//! `Table<SqliteDB, EmptyEntity>` first and wraps it as `from_table` does.
//! Ids stringify, matching `TableSource::Id`.

pub mod factory;
#[cfg(feature = "rhai")]
pub mod rhai_source;
pub mod source;
pub mod spec;

pub use factory::SqliteVistaFactory;
pub use source::SqliteTableShell;
pub use spec::{
    InheritBlock, SqliteColumnBlock, SqliteColumnExtras, SqliteTableBlock, SqliteTableExtras,
    SqliteVistaSpec,
};

use crate::sqlite::SqliteDB;

impl SqliteDB {
    /// Return a Vista factory bound to this SQLite data source.
    pub fn vista_factory(&self) -> SqliteVistaFactory {
        SqliteVistaFactory::new(self.clone())
    }
}
