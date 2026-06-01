pub mod window;

use vantage_expressions::Expression;

/// Trait for dialect-specific SELECT extensions beyond `Selectable`:
/// joins, HAVING, CTEs, and named windows.
///
/// Implemented by each backend's Select type (SqliteSelect, PostgresSelect, etc.)
pub trait SelectBuilder<V>: Clone {
    type Join;

    fn push_join(&mut self, join: Self::Join);
    fn push_having(&mut self, cond: Expression<V>);
    fn push_cte(&mut self, name: String, query: Expression<V>, recursive: bool);
}

/// Trait for constructing dialect-specific join clauses.
pub trait JoinBuilder<V>: Sized {
    fn make_inner(table: &str, alias: &str, on: Expression<V>) -> Self;
    fn make_left(table: &str, alias: &str, on: Expression<V>) -> Self;
}
