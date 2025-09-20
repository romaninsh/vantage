use std::fmt::Debug;

use crate::{Expr, Expression};

pub trait Selectable: Send + Sync + Debug + Into<Expression> {
    /// Specifies a source for a query. Depending on implementation, can be executed
    /// multiple times. If `source` is expression you might need to use alias.
    fn set_source(&mut self, source: impl Into<Expr>, alias: Option<String>);
    fn add_field(&mut self, field: impl Into<String>);
    fn add_expression(&mut self, expression: Expression, alias: Option<String>);
    fn add_where_condition(&mut self, condition: Expression);
    fn set_distinct(&mut self, distinct: bool);
    fn add_order_by(&mut self, field_or_expr: impl Into<Expr>, ascending: bool);
    fn add_group_by(&mut self, expression: Expression);
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

    // Default implementations for builder-style methods
    fn with_source(mut self, source: impl Into<Expr>) -> Self
    where
        Self: Sized,
    {
        Self::set_source(&mut self, source, None);
        self
    }

    fn with_source_as(mut self, source: impl Into<Expr>, alias: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        Self::set_source(&mut self, source, Some(alias.into()));
        self
    }

    fn with_condition(mut self, condition: Expression) -> Self
    where
        Self: Sized,
    {
        Self::add_where_condition(&mut self, condition);
        self
    }

    fn with_order(mut self, field_or_expr: impl Into<Expr>, ascending: bool) -> Self
    where
        Self: Sized,
    {
        Self::add_order_by(&mut self, field_or_expr, ascending);
        self
    }

    fn with_field(mut self, field: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        Self::add_field(&mut self, field);
        self
    }

    fn with_expression(mut self, expression: Expression, alias: Option<String>) -> Self
    where
        Self: Sized,
    {
        Self::add_expression(&mut self, expression, alias);
        self
    }
}
