use vantage_expressions::{Expression, Expressive, expr_any};

use crate::postgres::types::AnyPostgresType;
use crate::primitives::identifier::ident;

use super::{Expr, PostgresDelete};

impl PostgresDelete {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyPostgresType> for PostgresDelete {
    fn expr(&self) -> Expr {
        if self.conditions.is_empty() {
            return expr_any!("DELETE FROM {}", (ident(&self.table)));
        }

        let combined = Expression::from_vec(self.conditions.clone(), " AND ");
        expr_any!("DELETE FROM {} WHERE {}", (ident(&self.table)), (combined))
    }
}

impl From<PostgresDelete> for Expr {
    fn from(delete: PostgresDelete) -> Self {
        delete.expr()
    }
}
