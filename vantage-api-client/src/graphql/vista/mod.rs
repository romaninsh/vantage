//! Vista bridge for the GraphQL backend.
//!
//! Construct a `Vista` from a typed `Table<GraphqlApi, E>` via
//! `GraphqlApi::vista_factory().from_table(...)`. Metadata (columns, id
//! field, title fields, references) is harvested up front; the typed
//! table is then re-wrapped as `Table<GraphqlApi, EmptyEntity>` so the
//! universal Vista surface sees the same level of detail as the REST
//! bridge.
//!
//! A YAML spec builds the same way through `build_from_spec`, and its
//! `references:` traverse to other specs on the same endpoint.
//!
//! The shell only reads: it counts, lists and fetches by id, and
//! advertises ordering, search and paging where the root field takes them.
//! Writes are not supported, because mutation names and argument shapes
//! differ from one schema to the next.

pub mod factory;
mod lowering;
mod narrow;
mod paging;
mod source;
pub mod spec;
mod traversal;

pub use factory::GraphqlApiVistaFactory;
pub use lowering::GraphqlSpecResolver;
pub use source::GraphqlApiTableShell;
pub use spec::{
    GraphqlApiVistaSpec, GraphqlBlock, GraphqlColumnBlock, GraphqlColumnExtras, GraphqlTableExtras,
};

use crate::graphql::api::GraphqlApi;

impl GraphqlApi {
    /// Return a Vista factory bound to this GraphQL data source.
    pub fn vista_factory(&self) -> GraphqlApiVistaFactory {
        GraphqlApiVistaFactory::new(self.clone())
    }
}
