use std::fmt::Debug;

use super::expressive::Expressive;
use crate::{Expression, ExpressiveEnum};

/// Sort direction and null handling for ORDER BY clauses.
///
/// ```ignore
/// .with_order(ident("name"), Order::Asc)
/// .with_order(ident("score"), Order::Desc.nulls_last())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Order {
    pub ascending: bool,
    pub nulls: Option<Nulls>,
}

/// NULL placement in ORDER BY.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Nulls {
    First,
    Last,
}

#[allow(non_upper_case_globals)]
impl Order {
    pub const Asc: Order = Order {
        ascending: true,
        nulls: None,
    };
    pub const Desc: Order = Order {
        ascending: false,
        nulls: None,
    };

    pub fn nulls_last(self) -> Self {
        Self {
            nulls: Some(Nulls::Last),
            ..self
        }
    }

    pub fn nulls_first(self) -> Self {
        Self {
            nulls: Some(Nulls::First),
            ..self
        }
    }

    /// Returns the SQL suffix for this ordering, e.g. `""`, `" DESC"`, `" DESC NULLS LAST"`.
    pub fn suffix(&self) -> &'static str {
        match (self.ascending, self.nulls) {
            (true, None) => "",
            (false, None) => " DESC",
            (true, Some(Nulls::Last)) => " NULLS LAST",
            (true, Some(Nulls::First)) => " NULLS FIRST",
            (false, Some(Nulls::Last)) => " DESC NULLS LAST",
            (false, Some(Nulls::First)) => " DESC NULLS FIRST",
        }
    }
}

/// Unified protocol for building SELECT queries across different databases.
///
/// The `Selectable` trait provides a standardized interface for building SELECT-style
/// queries that work with databases supporting columns, conditions, ordering, limits,
/// and aggregations. This allows the same query building patterns to work across
/// SQL databases, SurrealDB, MongoDB, and other backends.
///
/// Implementations handle database-specific syntax while exposing a consistent API
/// for field selection, filtering, sorting, grouping, and aggregation operations.
/// The trait supports both mutable builder methods and fluent chainable methods.
pub trait Selectable<T>: Send + Sync + Debug + Clone + Expressive<T> {
    /// Sets the data source for the query (table name, subquery, etc.).
    fn set_source(&mut self, source: impl Into<SourceRef<T>>, alias: Option<String>);

    /// Adds a column name to the SELECT clause.
    fn add_field(&mut self, field: impl Into<String>);

    /// Adds a complex expression to the SELECT clause with optional alias.
    fn add_expression(&mut self, expression: impl Expressive<T>, alias: Option<String>);

    /// Adds a condition to the WHERE clause.
    fn add_where_condition(&mut self, condition: impl Expressive<T>);

    /// Sets whether the query should return distinct results.
    fn set_distinct(&mut self, distinct: bool);

    /// Adds an ORDER BY clause with direction and optional null handling.
    fn add_order_by(&mut self, expression: impl Expressive<T>, order: Order);

    /// Adds a GROUP BY clause.
    fn add_group_by(&mut self, expression: impl Expressive<T>);

    /// Sets LIMIT and OFFSET for result pagination.
    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>);

    /// Removes all fields from the SELECT clause.
    fn clear_fields(&mut self);

    /// Removes all WHERE conditions.
    fn clear_where_conditions(&mut self);

    /// Removes all ORDER BY clauses.
    fn clear_order_by(&mut self);

    /// Removes all GROUP BY clauses.
    fn clear_group_by(&mut self);

    /// Returns true if any fields have been added to SELECT clause.
    fn has_fields(&self) -> bool;

    /// Returns true if any WHERE conditions have been added.
    fn has_where_conditions(&self) -> bool;

    /// Returns true if any ORDER BY clauses have been added.
    fn has_order_by(&self) -> bool;

    /// Returns true if any GROUP BY clauses have been added.
    fn has_group_by(&self) -> bool;

    /// Returns true if DISTINCT mode is enabled.
    fn is_distinct(&self) -> bool;

    /// Returns the current LIMIT value, if set.
    fn get_limit(&self) -> Option<i64>;

    /// Returns the current OFFSET/SKIP value, if set.
    fn get_skip(&self) -> Option<i64>;

    /// Creates a COUNT(*) expression from this query configuration.
    fn as_count(&self) -> Expression<T>;

    /// Creates a SUM(column) expression from this query configuration.
    fn as_sum(&self, column: impl Expressive<T>) -> Expression<T>;

    /// Creates a MAX(column) expression from this query configuration.
    fn as_max(&self, column: impl Expressive<T>) -> Expression<T>;

    /// Creates a MIN(column) expression from this query configuration.
    fn as_min(&self, column: impl Expressive<T>) -> Expression<T>;

    // Default implementations for builder-style methods

    /// Builder pattern method identical to [`Self::set_source`] without alias.
    fn with_source(mut self, source: impl Into<SourceRef<T>>) -> Self
    where
        Self: Sized,
    {
        Self::set_source(&mut self, source, None);
        self
    }

    /// Builder pattern method identical to [`Self::set_source`] with alias.
    fn with_source_as(mut self, source: impl Into<SourceRef<T>>, alias: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        Self::set_source(&mut self, source, Some(alias.into()));
        self
    }

    /// Builder pattern method identical to [`Self::add_where_condition`].
    fn with_condition(mut self, condition: impl Expressive<T>) -> Self
    where
        Self: Sized,
    {
        Self::add_where_condition(&mut self, condition);
        self
    }

    /// Builder pattern method identical to [`Self::add_order_by`].
    fn with_order(mut self, expression: impl Expressive<T>, order: Order) -> Self
    where
        Self: Sized,
    {
        Self::add_order_by(&mut self, expression, order);
        self
    }

    /// Builder pattern method identical to [`Self::add_field`].
    fn with_field(mut self, field: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        Self::add_field(&mut self, field);
        self
    }

    /// Builder pattern method identical to [`Self::add_expression`].
    fn with_expression(mut self, expression: impl Expressive<T>, alias: Option<String>) -> Self
    where
        Self: Sized,
    {
        Self::add_expression(&mut self, expression, alias);
        self
    }

    /// Builder pattern method identical to [`Self::add_group_by`].
    fn with_group_by(mut self, expression: impl Expressive<T>) -> Self
    where
        Self: Sized,
    {
        Self::add_group_by(&mut self, expression);
        self
    }

    /// Builder pattern method identical to [`Self::set_distinct`].
    fn with_distinct(mut self, distinct: bool) -> Self
    where
        Self: Sized,
    {
        Self::set_distinct(&mut self, distinct);
        self
    }

    /// Builder pattern method identical to [`Self::set_limit`].
    fn with_limit(mut self, limit: Option<i64>, skip: Option<i64>) -> Self
    where
        Self: Sized,
    {
        Self::set_limit(&mut self, limit, skip);
        self
    }
}

/// A flexible type for source references that can be converted from various types.
pub struct SourceRef<T>(ExpressiveEnum<T>);

impl<T> SourceRef<T> {
    pub fn into_expressive_enum(self) -> ExpressiveEnum<T> {
        self.0
    }
}

impl<T> From<&str> for SourceRef<T>
where
    T: From<String>,
{
    fn from(value: &str) -> Self {
        SourceRef(ExpressiveEnum::Scalar(T::from(value.to_string())))
    }
}

impl<T> From<String> for SourceRef<T>
where
    T: From<String>,
{
    fn from(value: String) -> Self {
        SourceRef(ExpressiveEnum::Scalar(T::from(value)))
    }
}

impl<T> From<Expression<T>> for SourceRef<T> {
    fn from(value: Expression<T>) -> Self {
        SourceRef(ExpressiveEnum::Nested(value))
    }
}
