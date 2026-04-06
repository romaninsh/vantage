use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::mysql::types::AnyMysqlType;

use super::{Expr, MysqlDelete};

impl MysqlDelete {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyMysqlType> for MysqlDelete {
    fn expr(&self) -> Expr {
        if self.conditions.is_empty() {
            return Expression::new(format!("DELETE FROM `{}`", self.table), vec![]);
        }

        let combined = Expression::from_vec(self.conditions.clone(), " AND ");
        Expression::new(
            format!("DELETE FROM `{}` WHERE {{}}", self.table),
            vec![ExpressiveEnum::Nested(combined)],
        )
    }
}

impl From<MysqlDelete> for Expr {
    fn from(delete: MysqlDelete) -> Self {
        delete.expr()
    }
}
