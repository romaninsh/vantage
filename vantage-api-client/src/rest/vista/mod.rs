//! Vista bridge for the REST API backend.
//!
//! Construct a `Vista` from a typed `Table<RestApi, E>` via
//! `RestApi::vista_factory().from_table(...)`. The factory harvests
//! schema metadata (columns, id field, references) and wraps the
//! erased table in a `RestApiTableShell` that the universal Vista
//! surface drives.
//!
//! A YAML spec builds the same way through `build_from_spec`, and its
//! `references:` traverse to other models through the factory's registry
//! or an installed [`VistaResolver`](vantage_vista::VistaResolver).
//!
//! The shell only reads: it counts, lists and fetches by id, and
//! advertises absolute-offset windows and ordering where the API is
//! configured for them. Writes go through [`RestApi::http_request`].

pub mod factory;
mod source;
pub mod spec;

pub use factory::RestApiVistaFactory;
pub use source::RestApiTableShell;
pub use spec::{
    ApiColumnExtras, ApiReferenceExtras, ApiTableBlock, ApiTableExtras, RestApiVistaSpec,
};

use crate::RestApi;

impl RestApi {
    /// Return a Vista factory bound to this REST API data source.
    pub fn vista_factory(&self) -> RestApiVistaFactory {
        RestApiVistaFactory::new(self.clone())
    }
}
