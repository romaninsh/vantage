mod builder;
mod render;

use serde_json::Value as JsonValue;
use vantage_expressions::Expression;

type Expr = Expression<JsonValue>;

/// SQLite SELECT statement builder.
#[derive(Debug, Clone)]
pub struct SqliteSelect {
    pub fields: Vec<Expr>,
    pub from: Vec<Expr>,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub group_by: Vec<Expr>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

impl Default for SqliteSelect {
    fn default() -> Self {
        Self::new()
    }
}
