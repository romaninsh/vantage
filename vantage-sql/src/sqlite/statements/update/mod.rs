mod builder;
mod render;

use crate::sqlite::types::AnySqliteType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnySqliteType>;

/// SQLite UPDATE statement builder.
#[derive(Debug, Clone)]
pub struct SqliteUpdate {
    pub table: String,
    pub fields: IndexMap<String, AnySqliteType>,
    pub conditions: Vec<Expr>,
}
