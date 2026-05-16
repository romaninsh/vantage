//! Vantage adapters for HTTP-based API backends.
//!
//! Currently ships one protocol adapter:
//!
//! * [`rest`] — generic REST API client. Maps `Table<RestApi, E>` to
//!   `GET {base_url}/{table_name}` with eq-conditions and pagination
//!   peeled into the URL query string.
//!
//! A `graphql` sibling adapter is planned.

pub mod graphql;
pub mod rest;

pub use graphql::{
    AnyGraphqlType, FieldCondition, FilterDialect, GraphqlApi, GraphqlApiBuilder,
    GraphqlApiTableShell, GraphqlApiVistaFactory, GraphqlApiVistaSpec, GraphqlBlock,
    GraphqlColumnBlock, GraphqlColumnExtras, GraphqlCondition, GraphqlOp, GraphqlOperation,
    GraphqlSelect, GraphqlTableExtras, GraphqlType, GraphqlTypeVariants, NoGraphqlExtras,
    RenderedQuery,
};
pub use rest::{
    AnyTableShell, NoApiExtras, PaginationParams, ResponseShape, RestApi, RestApiBuilder,
    RestApiTableShell, RestApiVistaFactory, RestApiVistaSpec, eq_condition,
};
pub(crate) use rest::condition_to_query_param;
