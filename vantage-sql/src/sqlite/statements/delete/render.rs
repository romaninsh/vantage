use vantage_expressions::{Expression, Expressive, expr_any};

use crate::primitives::identifier::ident;
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
            return expr_any!("DELETE FROM {}", (ident(&self.table)));
        }

        let combined = Expression::from_vec(self.conditions.clone(), " AND ");
        expr_any!("DELETE FROM {} WHERE {}", (ident(&self.table)), (combined))
    }
}

impl From<SqliteDelete> for Expr {
    fn from(delete: SqliteDelete) -> Self {
        delete.expr()
    }
}
