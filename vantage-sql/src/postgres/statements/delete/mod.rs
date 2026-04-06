mod builder;
mod render;

use crate::postgres::types::AnyPostgresType;
use vantage_expressions::Expression;

type Expr = Expression<AnyPostgresType>;

/// PostgreSQL DELETE statement builder.
#[derive(Debug, Clone)]
pub struct PostgresDelete {
    pub table: String,
    pub conditions: Vec<Expr>,
}
