//! Vista bridge for the GraphQL backend.
//!
//! Construct a `Vista` from a typed `Table<GraphqlApi, E>` via
//! `GraphqlApi::vista_factory().from_table(...)`. Metadata (columns, id
//! field, title fields, references) is harvested up front; the typed
//! table is then re-wrapped as `Table<GraphqlApi, EmptyEntity>` so the
//! universal Vista surface sees the same level of detail as the REST
//! bridge.
//!
//! GraphQL is read-only at this stage — the shell advertises
//! `can_count` only. Writes will follow once the schema map (Phase 9)
//! describes mutation field names per-table.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::GraphqlApiVistaFactory;
pub use source::GraphqlApiTableShell;
pub use spec::{GraphqlApiVistaSpec, NoGraphqlExtras};

use crate::graphql::api::GraphqlApi;

impl GraphqlApi {
    /// Return a Vista factory bound to this GraphQL data source.
    pub fn vista_factory(&self) -> GraphqlApiVistaFactory {
        GraphqlApiVistaFactory::new(self.clone())
    }
}
