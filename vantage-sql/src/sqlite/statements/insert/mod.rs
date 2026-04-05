mod builder;
mod render;

use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_expressions::Expression;

type Expr = Expression<JsonValue>;

/// SQLite INSERT statement builder.
#[derive(Debug, Clone)]
pub struct SqliteInsert {
    pub table: String,
    pub fields: IndexMap<String, JsonValue>,
}
