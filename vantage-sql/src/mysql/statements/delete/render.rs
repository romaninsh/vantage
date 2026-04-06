use vantage_expressions::{Expression, Expressive, expr_any};

use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;

use super::{Expr, MysqlDelete};

impl MysqlDelete {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyMysqlType> for MysqlDelete {
    fn expr(&self) -> Expr {
        if self.conditions.is_empty() {
            return expr_any!("DELETE FROM {}", (ident(&self.table)));
        }

        let combined = Expression::from_vec(self.conditions.clone(), " AND ");
        expr_any!("DELETE FROM {} WHERE {}", (ident(&self.table)), (combined))
    }
}

impl From<MysqlDelete> for Expr {
    fn from(delete: MysqlDelete) -> Self {
        delete.expr()
    }
}
