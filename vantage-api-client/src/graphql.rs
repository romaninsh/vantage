// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! GraphQL adapter for Vantage.
//!
//! Talks to a GraphQL endpoint over HTTP. Unlike the REST sibling, this
//! adapter has its own query language (rendered via [`GraphqlSelect`]),
//! structured filter conditions ([`GraphqlCondition`]), mandatory field
//! projection, and nested selection sets for relationships — closer in
//! shape to the MongoDB or SQL backends than to REST.
//!
//! Schemas are maintained locally (no introspection); type information
//! flows through the [`GraphqlType`] system declared in [`types`]. The
//! [`vista`] module bridges tables and YAML specs into Vista.

mod api;
mod condition;
pub(crate) mod impls;
pub(crate) mod name;
mod operation;
mod select;
pub mod types;
pub mod vista;

pub use api::{CountMode, GraphqlApi, GraphqlApiBuilder, Supports};
pub use condition::{FieldCondition, FilterDialect, GraphqlCondition, GraphqlOp};
pub use operation::GraphqlOperation;
pub use select::{GraphqlSelect, RenderedQuery};
pub use types::{AnyGraphqlType, GraphqlType, GraphqlTypeVariants};
pub use vista::{
    GraphqlApiTableShell, GraphqlApiVistaFactory, GraphqlApiVistaSpec, GraphqlBlock,
    GraphqlColumnBlock, GraphqlColumnExtras, GraphqlTableExtras,
};
