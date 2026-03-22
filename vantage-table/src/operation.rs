//! Generic operation trait for building conditions on table columns.
//!
//! Each persistence backend provides its own implementation.
//! CSV uses structured parameters evaluated in memory.
//! SQL/SurrealDB render as query syntax.

use vantage_expressions::{Expression, Expressive};

/// Template markers for condition operations.
/// Backends match on these in their condition evaluators.
pub const OP_EQ: &str = "{} = {}";
pub const OP_IN: &str = "{} IN ({})";

/// Trait for building condition expressions from column references.
///
/// Each backend implements this for its column types. The trait defines
/// the interface; backends provide the implementation.
pub trait Operation<T>: Expressive<T> {
    /// Creates an equality condition: field = value
    fn eq(&self, value: T) -> Expression<T>;

    /// Creates a membership condition: field IN (values_expression)
    fn in_(&self, values: Expression<T>) -> Expression<T>;
}
