//! # SurrealDB Query Builder
//!
//! doc wip

pub mod join_query;
pub mod query_conditions;
pub mod query_source;
pub mod query_type;

use indexmap::IndexMap;
use serde_json::Value;
use vantage_expressions::OwnedExpression;

use join_query::JoinQuery;
use query_conditions::QueryConditions;
use query_source::QuerySource;
use query_type::QueryType;

/// Generic SurrealDB query builder
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::query::Query;
///
/// // doc wip
/// ```
#[derive(Debug, Clone)]
pub struct Query {
    /// doc wip
    table: QuerySource,
    /// doc wip
    with: IndexMap<String, QuerySource>,
    /// doc wip
    distinct: bool,
    /// doc wip
    query_type: QueryType,
    /// doc wip
    fields: IndexMap<Option<String>, OwnedExpression>,
    /// doc wip
    set_fields: IndexMap<String, Value>,

    /// doc wip
    where_conditions: QueryConditions,
    /// doc wip
    having_conditions: QueryConditions,
    /// doc wip
    joins: Vec<JoinQuery>,

    /// doc wip
    skip_items: Option<i64>,
    /// doc wip
    limit_items: Option<i64>,

    /// doc wip
    group_by: Vec<OwnedExpression>,
    /// doc wip
    order_by: Vec<OwnedExpression>,
}
