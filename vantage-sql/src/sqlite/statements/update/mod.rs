mod builder;
mod render;

use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_expressions::Expression;

type Expr = Expression<JsonValue>;

/// SQLite UPDATE statement builder.
#[derive(Debug, Clone)]
pub struct SqliteUpdate {
    pub table: String,
    pub fields: IndexMap<String, JsonValue>,
    pub conditions: Vec<Expr>,
}
