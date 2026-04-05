use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::sqlite::types::AnySqliteType;

use super::{Expr, SqliteDelete};

impl SqliteDelete {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySqliteType> for SqliteDelete {
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

impl From<SqliteDelete> for Expr {
    fn from(delete: SqliteDelete) -> Self {
        delete.expr()
    }
}
