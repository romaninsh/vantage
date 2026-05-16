//! Vantage adapters for HTTP-based API backends.
//!
//! Two protocol adapters live side by side:
//!
//! * [`rest`] — generic REST API client. Maps `Table<RestApi, E>` to
//!   `GET {base_url}/{table_name}` with eq-conditions and pagination
//!   peeled into the URL query string.
//! * [`graphql`] — GraphQL API client. Renders typed query documents
//!   with inline filters + variable-typed pagination, supports both
//!   Hasura and flat-argument (e.g. SpaceX) dialects, and bridges
//!   into Vista via a YAML schema map.
//!
//! See [`prelude`] for a one-stop import.

pub mod graphql;
pub mod prelude;
pub mod rest;

pub use graphql::{
    AnyGraphqlType, FieldCondition, FilterDialect, GraphqlApi, GraphqlApiBuilder,
    GraphqlApiTableShell, GraphqlApiVistaFactory, GraphqlApiVistaSpec, GraphqlBlock,
    GraphqlColumnBlock, GraphqlColumnExtras, GraphqlCondition, GraphqlOp, GraphqlOperation,
    GraphqlSelect, GraphqlTableExtras, GraphqlType, GraphqlTypeVariants, NoGraphqlExtras,
    RenderedQuery,
};
pub(crate) use rest::condition_to_query_param;
pub use rest::{
    AnyTableShell, NoApiExtras, PaginationParams, ResponseShape, RestApi, RestApiBuilder,
    RestApiTableShell, RestApiVistaFactory, RestApiVistaSpec, eq_condition,
};
