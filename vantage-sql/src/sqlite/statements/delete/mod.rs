mod builder;
mod render;

use serde_json::Value as JsonValue;
use vantage_expressions::Expression;

type Expr = Expression<JsonValue>;

/// SQLite DELETE statement builder.
#[derive(Debug, Clone)]
pub struct SqliteDelete {
    pub table: String,
    pub conditions: Vec<Expr>,
}
