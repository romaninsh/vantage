mod builder;
mod render;

use crate::mysql::types::AnyMysqlType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnyMysqlType>;

/// MySQL INSERT statement builder.
#[derive(Debug, Clone)]
pub struct MysqlInsert {
    pub table: String,
    pub fields: IndexMap<String, AnyMysqlType>,
}
