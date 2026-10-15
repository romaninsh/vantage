// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! REST API adapter for Vantage.
//!
//! Wraps an HTTP JSON endpoint as a `TableSource`. Conditions on tables
//! become URL query parameters, pagination is appended, and responses are
//! parsed against a configurable [`ResponseShape`]. CBOR values flow
//! end-to-end, bridging cleanly into the Vista universal surface.
//!
//! See also: the [`graphql`](crate::graphql) sibling module for a
//! query-language-based HTTP API adapter.

mod api;
mod operation;
mod table_source;
pub mod vista;

pub use api::{
    FilterStrategy, OrderingParams, PaginationMode, PaginationParams, ResponseShape, RestApi,
    RestApiBuilder,
};
pub use operation::eq_condition;
pub(crate) use operation::{cbor_to_query_string, condition_to_query_param, placeholders};
pub(crate) use table_source::row_fields;
pub use vista::{
    ApiColumnExtras, ApiReferenceExtras, ApiTableBlock, ApiTableExtras, RestApiTableShell,
    RestApiVistaFactory, RestApiVistaSpec,
};
