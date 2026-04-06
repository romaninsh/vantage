use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::postgres::types::AnyPostgresType;

use super::{Expr, PostgresDelete};

impl PostgresDelete {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyPostgresType> for PostgresDelete {
    fn expr(&self) -> Expr {
        if self.conditions.is_empty() {
            return Expression::new(format!("DELETE FROM \"{}\"", self.table), vec![]);
        }

        let combined = Expression::from_vec(self.conditions.clone(), " AND ");
        Expression::new(
            format!("DELETE FROM \"{}\" WHERE {{}}", self.table),
            vec![ExpressiveEnum::Nested(combined)],
        )
    }
}

impl From<PostgresDelete> for Expr {
    fn from(delete: PostgresDelete) -> Self {
        delete.expr()
    }
}
