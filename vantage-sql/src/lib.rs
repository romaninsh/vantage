pub mod condition;
pub mod prelude;
pub mod primitives;

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
use {
    vantage_core::Result,
    vantage_expressions::{Expression, expr_any},
    vantage_table::{table::Table, traits::table_source::TableSource},
    vantage_types::Entity,
};

// Re-export so that macros (sqlite_expr!, sql_expr!, etc.) resolve
// without downstream crates needing a direct vantage-expressions dependency.
pub use vantage_expressions;
pub(crate) mod types;

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod declared;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod list_aggregate;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod set_writes;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod sql_exec;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod sql_tx;
#[cfg(all(
    feature = "vista",
    any(feature = "sqlite", feature = "postgres", feature = "mysql")
))]
mod sql_vista;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
mod table_writes;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "mysql")]
pub mod mysql;

#[cfg(feature = "rhai")]
pub mod rhai_engine;

/// The `LIKE` operand for a substring match: `%text%` with the wildcards and
/// the escape itself escaped, so a `_` or `%` in what the user typed matches
/// itself. Shared by quicksearch and by the vista `FilterOp::Like`, which is
/// what keeps the two spelling the same pattern.
pub(crate) fn like_pattern_str(text: &str) -> String {
    let escaped = text
        .replace('$', "$$")
        .replace('%', "$%")
        .replace('_', "$_");
    format!("%{escaped}%")
}

/// What a WHERE says for column `name`: an expression column's expression
/// in parentheses, since SQL has no SELECT alias in WHERE, else `stored`.
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
pub(crate) fn where_subject<T, E>(
    table: &Table<T, E>,
    name: &str,
    stored: Expression<T::Value>,
) -> Result<Expression<T::Value>>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    Ok(match table.try_expression_column(name)? {
        Some(expr) => expr_any!("({})", (expr)),
        None => stored,
    })
}

/// [`where_subject`] for quicksearch over column `name`, its stored form
/// quoted. An expression whose callback fails falls back to the name: the
/// same read's SELECT builds that expression and fails with its error.
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
pub(crate) fn search_subject<T, E>(table: &Table<T, E>, name: &str) -> Expression<T::Value>
where
    T: set_writes::SetWrites,
    E: Entity<T::Value>,
{
    where_subject(table, name, T::quoted(name)).unwrap_or_else(|_| T::quoted(name))
}

/// [`like_pattern_str`] over a filter operand, which may be any scalar.
#[cfg(feature = "vista")]
pub(crate) fn like_pattern(value: &ciborium::Value) -> String {
    like_pattern_str(&vantage_vista::operand_text(value))
}
