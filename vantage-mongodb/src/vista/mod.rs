//! Vista bridge for the MongoDB backend.
//!
//! Construct a `Vista` from a typed `Table<MongoDB, E>` via
//! `MongoDB::vista_factory()`, or from a YAML spec via
//! `MongoVistaFactory::from_yaml`. The YAML path builds a
//! `Table<MongoDB, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path.
//!
//! BSON values surface to consumers as CBOR; ObjectIds stringify at the
//! `Vista` boundary (matching how `MongoId` already serializes for HTTP).

pub mod cbor;
pub mod factory;
pub mod source;
pub mod spec;

pub use cbor::{bson_to_cbor, cbor_to_bson};
pub use factory::MongoVistaFactory;
pub use source::MongoTableShell;
pub use spec::{MongoBlock, MongoColumnBlock, MongoColumnExtras, MongoTableExtras, MongoVistaSpec};

use crate::mongodb::MongoDB;

impl MongoDB {
    /// Return a Vista factory bound to this MongoDB data source.
    pub fn vista_factory(&self) -> MongoVistaFactory {
        MongoVistaFactory::new(self.clone())
    }
}
