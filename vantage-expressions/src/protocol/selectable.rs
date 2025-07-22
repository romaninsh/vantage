use std::fmt::Debug;

use crate::{Expr, OwnedExpression};

pub trait Selectable: Send + Sync + Debug {
    /// Specifies a source for a query. Depending on implementation, can be executed
    /// multiple times. If `source` is expression you might need to use alias.
    fn set_source(&mut self, source: impl Into<Expr>, alias: Option<String>);
    fn add_field(&mut self, field: String);
    fn add_expression(&mut self, expression: OwnedExpression, alias: Option<String>);
    fn add_where_condition(&mut self, condition: OwnedExpression);
    fn set_distinct(&mut self, distinct: bool);
    fn add_order_by(&mut self, expression: OwnedExpression, ascending: bool);
    fn add_group_by(&mut self, expression: OwnedExpression);
    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>);
    fn clear_fields(&mut self);
    fn clear_where_conditions(&mut self);
    fn clear_order_by(&mut self);
    fn clear_group_by(&mut self);
    fn has_fields(&self) -> bool;
    fn has_where_conditions(&self) -> bool;
    fn has_order_by(&self) -> bool;
    fn has_group_by(&self) -> bool;
    fn is_distinct(&self) -> bool;
    fn get_limit(&self) -> Option<i64>;
    fn get_skip(&self) -> Option<i64>;
}
