pub mod expressive;
pub mod join_query;
pub mod query_conditions;
pub mod query_source;
pub mod query_type;

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;
use vantage_expressions::{Expressive, LazyExpression};

use join_query::JoinQuery;
use query_conditions::QueryConditions;
use query_source::QuerySource;
use query_type::QueryType;

#[derive(Debug, Clone)]
pub struct Query {
    table: QuerySource,
    with: IndexMap<String, QuerySource>,
    distinct: bool,
    query_type: QueryType,
    fields: IndexMap<Option<String>, Arc<Box<dyn Expressive>>>,
    set_fields: IndexMap<String, Value>,

    where_conditions: QueryConditions,
    having_conditions: QueryConditions,
    joins: Vec<JoinQuery>,

    skip_items: Option<i64>,
    limit_items: Option<i64>,

    group_by: Vec<LazyExpression>,
    order_by: Vec<LazyExpression>,
}
