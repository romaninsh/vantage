mod builder;
mod render;

use crate::sqlite::types::AnySqliteType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnySqliteType>;

/// SQLite INSERT statement builder.
#[derive(Debug, Clone)]
pub struct SqliteInsert {
    pub table: String,
    pub fields: IndexMap<String, AnySqliteType>,
}
