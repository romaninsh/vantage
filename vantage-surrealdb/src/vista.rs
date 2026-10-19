//! The SurrealDB Vista driver: build a `Vista` from a typed
//! `Table<SurrealDB, E>` or a YAML spec through `SurrealDB::vista_factory()`.
//! Values cross as CBOR unchanged; `Thing` ids as `"table:id"`.

pub mod factory;
#[cfg(feature = "rhai")]
pub mod rhai_source;
pub mod source;
pub mod spec;

pub use factory::SurrealVistaFactory;
pub use source::SurrealTableShell;
#[cfg(feature = "rhai")]
pub use source::register_rule_conditions;
pub use spec::{
    InheritBlock, SurrealColumnBlock, SurrealColumnExtras, SurrealTableBlock, SurrealTableExtras,
    SurrealVistaSpec,
};

use crate::surrealdb::SurrealDB;

impl SurrealDB {
    /// Return a Vista factory bound to this SurrealDB data source.
    pub fn vista_factory(&self) -> SurrealVistaFactory {
        SurrealVistaFactory::new(self.clone())
    }
}
