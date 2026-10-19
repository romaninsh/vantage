//! Vista bridge for the MySQL backend: `MysqlDB::vista_factory()` builds a
//! `Vista` from a typed `Table<MysqlDB, E>` or, through
//! `VistaFactory::from_yaml`, from a YAML spec. The YAML path builds a
//! `Table<MysqlDB, EmptyEntity>` first and wraps it as `from_table` does.
//! Ids stringify, matching `TableSource::Id`.

pub mod factory;
#[cfg(feature = "rhai")]
pub mod rhai_source;
pub mod source;
pub mod spec;

pub use factory::MysqlVistaFactory;
pub use source::MysqlTableShell;
#[cfg(feature = "rhai")]
pub use source::register_rule_conditions;
pub use spec::{
    InheritBlock, MysqlColumnBlock, MysqlColumnExtras, MysqlRefBlock, MysqlReferenceExtras,
    MysqlTableBlock, MysqlTableExtras, MysqlVistaSpec,
};

use crate::mysql::MysqlDB;

impl MysqlDB {
    /// Return a Vista factory bound to this MySQL data source.
    pub fn vista_factory(&self) -> MysqlVistaFactory {
        MysqlVistaFactory::new(self.clone())
    }
}
