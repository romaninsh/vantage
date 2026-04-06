mod builder;
mod render;

use crate::mysql::types::AnyMysqlType;
use vantage_expressions::Expression;

type Expr = Expression<AnyMysqlType>;

/// MySQL DELETE statement builder.
#[derive(Debug, Clone)]
pub struct MysqlDelete {
    pub table: String,
    pub conditions: Vec<Expr>,
}
