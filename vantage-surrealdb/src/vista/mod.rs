//! Vista bridge for the SurrealDB backend.
//!
//! Construct a `Vista` from a typed `Table<SurrealDB, E>` via
//! `SurrealDB::vista_factory()`, or from a YAML spec via
//! `SurrealVistaFactory::from_yaml`. The YAML path builds a
//! `Table<SurrealDB, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path.
//!
//! `AnySurrealType` already wraps `ciborium::Value`, so the boundary
//! translation is a passthrough; `Thing` ids stringify as `"table:id"`.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::SurrealVistaFactory;
pub use source::SurrealTableShell;
pub use spec::{
    SurrealColumnBlock, SurrealColumnExtras, SurrealTableBlock, SurrealTableExtras,
    SurrealVistaSpec,
};

use crate::surrealdb::SurrealDB;

impl SurrealDB {
    /// Return a Vista factory bound to this SurrealDB data source.
    pub fn vista_factory(&self) -> SurrealVistaFactory {
        SurrealVistaFactory::new(self.clone())
    }
}
