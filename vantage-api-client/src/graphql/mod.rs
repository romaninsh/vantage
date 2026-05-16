//! GraphQL adapter for Vantage.
//!
//! Talks to a GraphQL endpoint over HTTP. Unlike the REST sibling, this
//! adapter has its own query language (rendered via `GraphqlSelect`),
//! structured filter conditions, mandatory field projection, and
//! nested selection sets for relationships — closer in shape to the
//! MongoDB or SQL backends than to REST.
//!
//! Schemas are maintained locally (no introspection); type information
//! flows through the `GraphqlType` system declared in `types`.

pub mod api;
pub mod impls;
pub mod types;

pub use api::{GraphqlApi, GraphqlApiBuilder};
pub use types::{AnyGraphqlType, GraphqlType, GraphqlTypeVariants};
