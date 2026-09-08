pub mod condition;
pub mod prelude;
pub mod primitives;

// Re-export so that macros (sqlite_expr!, sql_expr!, etc.) resolve
// without downstream crates needing a direct vantage-expressions dependency.
pub use vantage_expressions;
pub(crate) mod types;

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
/// itself. Shared by quicksearch and by
/// [`FilterOp::Like`](vantage_vista::FilterOp::Like), which is what keeps the
/// two spelling the same pattern.
pub(crate) fn like_pattern_str(text: &str) -> String {
    let escaped = text
        .replace('$', "$$")
        .replace('%', "$%")
        .replace('_', "$_");
    format!("%{escaped}%")
}

/// [`like_pattern_str`] over a filter operand, which may be any scalar.
pub(crate) fn like_pattern(value: &ciborium::Value) -> String {
    let text = match value {
        ciborium::Value::Text(s) => s.clone(),
        ciborium::Value::Integer(i) => i128::from(*i).to_string(),
        ciborium::Value::Float(f) => f.to_string(),
        ciborium::Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    };
    like_pattern_str(&text)
}
