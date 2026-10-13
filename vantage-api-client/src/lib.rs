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
//! Every type a caller needs is re-exported here; see [`prelude`] for a
//! one-stop import. Requests go through `vantage-api-pool`'s
//! [`ResilientClient`], whose types are re-exported too. The HTTP client
//! is `reqwest` 0.12, which appears in the public API
//! ([`RestApiBuilder::http_client`], [`RestApi::http_request`]).

#![warn(missing_docs)]

pub mod graphql;
pub mod prelude;
pub mod rest;
mod row_keys;
mod transport;

pub use vantage_api_pool::resilient::{
    AuthFuture, AuthRefresher, BreakerMode, BreakerState, CallPolicy, ClientError, ErrorKind,
    ResilientClient, ResilientClientBuilder, RetryMode, RetryPolicy, TransportEvent,
    TransportObserver,
};
pub use vantage_core::Priority;

pub use graphql::{
    AnyGraphqlType, CountMode, FieldCondition, FilterDialect, GraphqlApi, GraphqlApiBuilder,
    GraphqlApiTableShell, GraphqlApiVistaFactory, GraphqlApiVistaSpec, GraphqlBlock,
    GraphqlColumnBlock, GraphqlColumnExtras, GraphqlCondition, GraphqlOp, GraphqlOperation,
    GraphqlSelect, GraphqlTableExtras, GraphqlType, GraphqlTypeVariants, RenderedQuery, Supports,
};
pub use rest::{
    ApiColumnExtras, ApiReferenceExtras, ApiTableBlock, ApiTableExtras, FilterStrategy,
    OrderingParams, PaginationMode, PaginationParams, ResponseShape, RestApi, RestApiBuilder,
    RestApiTableShell, RestApiVistaFactory, RestApiVistaSpec, eq_condition,
};
pub(crate) use rest::{cbor_to_query_string, condition_to_query_param};
