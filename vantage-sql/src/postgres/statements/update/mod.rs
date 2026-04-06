mod builder;
mod render;

use crate::postgres::types::AnyPostgresType;
use indexmap::IndexMap;
use vantage_expressions::Expression;

type Expr = Expression<AnyPostgresType>;

/// PostgreSQL UPDATE statement builder.
#[derive(Debug, Clone)]
pub struct PostgresUpdate {
    pub table: String,
    pub fields: IndexMap<String, AnyPostgresType>,
    pub conditions: Vec<Expr>,
}
