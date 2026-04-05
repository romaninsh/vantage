mod builder;
mod render;

use crate::sqlite::types::AnySqliteType;
use vantage_expressions::Expression;

type Expr = Expression<AnySqliteType>;

/// SQLite DELETE statement builder.
#[derive(Debug, Clone)]
pub struct SqliteDelete {
    pub table: String,
    pub conditions: Vec<Expr>,
}
