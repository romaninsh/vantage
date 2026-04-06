mod builder;
mod render;

use crate::mysql::types::AnyMysqlType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnyMysqlType>;

/// MySQL UPDATE statement builder.
#[derive(Debug, Clone)]
pub struct MysqlUpdate {
    pub table: String,
    pub fields: IndexMap<String, AnyMysqlType>,
    pub conditions: Vec<Expr>,
}
