mod impls;
mod render;

use crate::sqlite::types::AnySqliteType;
use vantage_expressions::Expression;

type Expr = Expression<AnySqliteType>;

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

impl SqliteSelect {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            from: Vec::new(),
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
        }
    }
}

impl Default for SqliteSelect {
    fn default() -> Self {
        Self::new()
    }
}
