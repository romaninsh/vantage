//! REST API adapter for Vantage.
//!
//! Wraps an HTTP JSON endpoint as a `TableSource`. Conditions on tables
//! become URL query parameters, pagination is appended, and responses are
//! parsed against a configurable [`ResponseShape`]. CBOR values flow
//! end-to-end, bridging cleanly into the Vista universal surface.
//!
//! See also: the `graphql` sibling module for a query-language-based
//! HTTP API adapter.

pub mod api;
pub mod operation;
pub mod table_source;
pub mod vista;

pub use api::{PaginationParams, ResponseShape, RestApi, RestApiBuilder};
pub use operation::eq_condition;
pub(crate) use operation::condition_to_query_param;
pub use vista::{
    AnyTableShell, NoApiExtras, RestApiTableShell, RestApiVistaFactory, RestApiVistaSpec,
};
