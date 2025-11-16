use std::fmt::Debug;

use super::expressive::Expressive;
use crate::{Expression, ExpressiveEnum};

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
    ///
    /// Implementation should store the source and optionally an alias. The generated
    /// query should include this in the FROM clause (SQL) or equivalent construct.
    /// If called multiple times, behavior is implementation-defined (replace or join).
    fn set_source(&mut self, source: impl Into<SourceRef<T>>, alias: Option<String>);

    /// Adds a column name to the SELECT clause.
    ///
    /// Implementation should append this field to the list of selected columns.
    /// Multiple calls should accumulate fields. The field will be rendered as-is
    /// in the query (e.g., `SELECT field1, field2 FROM table`).
    fn add_field(&mut self, field: impl Into<String>);

    /// Adds a complex expression to the SELECT clause with optional alias.
    ///
    /// Implementation should append this expression to the selected fields.
    /// If alias is provided, render as `expression AS alias`. This allows
    /// calculated fields, function calls, or subqueries in the SELECT clause.
    fn add_expression(&mut self, expression: Expression<T>, alias: Option<String>);

    /// Adds a condition to the WHERE clause.
    ///
    /// Implementation should append this condition to existing WHERE conditions,
    /// typically joining with AND. Multiple calls should accumulate conditions
    /// (e.g., `WHERE cond1 AND cond2 AND cond3`).
    fn add_where_condition(&mut self, condition: Expression<T>);

    /// Sets whether the query should return distinct results.
    ///
    /// Implementation should add DISTINCT keyword (or equivalent) to the query
    /// when `distinct` is true, remove it when false. Should affect the entire
    /// result set to eliminate duplicate rows.
    fn set_distinct(&mut self, distinct: bool);

    /// Adds an ORDER BY clause with direction.
    ///
    /// Implementation should append to existing ordering clauses. Multiple calls
    /// should accumulate (e.g., `ORDER BY expr1 ASC, expr2 DESC`). The `ascending`
    /// parameter controls sort direction (true = ASC, false = DESC).
    fn add_order_by(&mut self, expression: Expression<T>, ascending: bool);

    /// Adds a GROUP BY clause.
    ///
    /// Implementation should append to existing GROUP BY expressions. Multiple
    /// calls should accumulate (e.g., `GROUP BY expr1, expr2`). Used for
    /// aggregation queries and typically requires aggregate functions in SELECT.
    fn add_group_by(&mut self, expression: Expression<T>);

    /// Sets LIMIT and OFFSET for result pagination.
    ///
    /// Implementation should configure result limiting. `limit` controls maximum
    /// rows returned, `skip` controls how many rows to skip (offset). Both None
    /// means no limit. Only limit means limit without offset.
    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>);

    /// Removes all fields from the SELECT clause.
    ///
    /// Implementation should clear the field list, typically resulting in
    /// SELECT * behavior if no fields remain.
    fn clear_fields(&mut self);

    /// Removes all WHERE conditions.
    ///
    /// Implementation should clear all WHERE clause conditions, resulting
    /// in an unfiltered query.
    fn clear_where_conditions(&mut self);

    /// Removes all ORDER BY clauses.
    ///
    /// Implementation should clear ordering, resulting in database-default
    /// result ordering.
    fn clear_order_by(&mut self);

    /// Removes all GROUP BY clauses.
    ///
    /// Implementation should clear grouping, converting back to non-aggregated query.
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
    ///
    /// Implementation should generate a query that counts matching rows instead
    /// of returning the actual data. Typically wraps the current query or
    /// converts SELECT fields to COUNT(*).
    fn as_count(&self) -> Expression<T>;

    /// Creates a SUM(column) expression from this query configuration.
    ///
    /// Implementation should generate a query that sums the specified column
    /// for all matching rows. Should preserve WHERE conditions but typically
    /// ignore field selections.
    fn as_sum(&self, column: Expression<T>) -> Expression<T>;

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
    fn with_condition(mut self, condition: Expression<T>) -> Self
    where
        Self: Sized,
    {
        Self::add_where_condition(&mut self, condition);
        self
    }

    /// Builder pattern method identical to [`Self::add_order_by`].
    fn with_order(mut self, expression: Expression<T>, ascending: bool) -> Self
    where
        Self: Sized,
    {
        Self::add_order_by(&mut self, expression, ascending);
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
    fn with_expression(mut self, expression: Expression<T>, alias: Option<String>) -> Self
    where
        Self: Sized,
    {
        Self::add_expression(&mut self, expression, alias);
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
///
/// `SourceRef` wraps different source types (strings, expressions) into a unified
/// format that can be used with the [`Selectable`] trait. It automatically converts
/// table names, subqueries, and other source references into the appropriate
/// [`ExpressiveEnum`] format for database-specific query builders.
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
