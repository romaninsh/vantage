mod builder;
mod render;

use crate::postgres::types::AnyPostgresType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnyPostgresType>;

/// PostgreSQL INSERT statement builder.
#[derive(Debug, Clone)]
pub struct PostgresInsert {
    pub table: String,
    pub fields: IndexMap<String, AnyPostgresType>,
}
