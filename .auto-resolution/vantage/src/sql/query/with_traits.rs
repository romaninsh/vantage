use std::sync::Arc;

use serde_json::Value;

use crate::prelude::*;

use super::{QueryConditions, QuerySource, QueryType};

/// Implementation of object-safe Query. All the methods
/// in form "query.with_condition()" are implemented
/// in Query struct instead
pub trait SqlQuery {
    fn set_distinct(&mut self, distinct: bool);
    fn set_table(&mut self, table: &str, alias: Option<String>);
    fn add_with(&mut self, alias: String, subquery: QuerySource);
    fn set_source(&mut self, source: QuerySource);
    fn set_type(&mut self, query_type: QueryType);
    fn add_field(&mut self, name: Option<String>, column: Arc<Box<dyn SqlField>>);
    fn get_where_conditions_mut(&mut self) -> &mut QueryConditions;
    fn get_having_conditions_mut(&mut self) -> &mut QueryConditions;
    fn add_join(&mut self, join: JoinQuery);
    fn add_group_by(&mut self, group_by: Expression);
    fn add_order_by(&mut self, order_by: Expression);
    fn add_limit(&mut self, limit: Option<i64>);
    fn add_skip(&mut self, skip: Option<i64>);
    fn set_field_value(&mut self, field: &str, value: Value);
}
