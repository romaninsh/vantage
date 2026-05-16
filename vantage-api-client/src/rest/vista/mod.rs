//! Vista bridge for the REST API backend.
//!
//! Construct a `Vista` from a typed `Table<RestApi, E>` via
//! `RestApi::vista_factory().from_table(...)`. The factory harvests
//! schema metadata (columns, id field, references) and wraps the
//! erased table in a `RestApiTableShell` that the universal Vista
//! surface drives.
//!
//! REST APIs are read-only at this stage — the shell advertises only
//! `can_count`.

pub mod any_shell;
pub mod factory;
pub mod source;
pub mod spec;

pub use any_shell::AnyTableShell;
pub use factory::{ModelResolver, RestApiVistaFactory};
pub use source::RestApiTableShell;
pub use spec::{
    ApiColumnExtras, ApiReferenceExtras, ApiTableBlock, ApiTableExtras, NoApiExtras,
    RestApiVistaSpec,
};

use crate::RestApi;

impl RestApi {
    /// Return a Vista factory bound to this REST API data source.
    pub fn vista_factory(&self) -> RestApiVistaFactory {
        RestApiVistaFactory::new(self.clone())
    }
}
