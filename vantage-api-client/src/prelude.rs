//! Convenience prelude — `use vantage_api_client::prelude::*;` pulls in
//! the public surface of both adapter modules in one shot.
//!
//! Designed for application code that already knows which protocol it
//! wants. If you want one protocol only, prefer importing from
//! [`crate::rest`] or [`crate::graphql`] directly.

pub use crate::graphql::{
    AnyGraphqlType, CountMode, FieldCondition, FilterDialect, GraphqlApi, GraphqlApiBuilder,
    GraphqlApiTableShell, GraphqlApiVistaFactory, GraphqlApiVistaSpec, GraphqlBlock,
    GraphqlColumnBlock, GraphqlColumnExtras, GraphqlCondition, GraphqlOp, GraphqlOperation,
    GraphqlSelect, GraphqlTableExtras, GraphqlType, GraphqlTypeVariants, RenderedQuery, Supports,
};
pub use crate::rest::{
    ApiTableBlock, ApiTableExtras, FilterStrategy, OrderingParams, PaginationMode,
    PaginationParams, ResponseShape, RestApi, RestApiBuilder, RestApiTableShell,
    RestApiVistaFactory, RestApiVistaSpec, eq_condition,
};
