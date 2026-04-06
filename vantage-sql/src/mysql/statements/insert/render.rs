use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;

use super::{Expr, MysqlInsert};

impl MysqlInsert {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyMysqlType> for MysqlInsert {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return expr_any!("INSERT INTO {} () VALUES ()", (ident(&self.table)));
        }

        let columns: Vec<Expr> = self.fields.keys().map(|k| ident(k).expr()).collect();
        let cols = Expression::from_vec(columns, ", ");

        let values: Vec<Expr> = self
            .fields
            .values()
            .map(|v| Expression::new("{}", vec![ExpressiveEnum::Scalar(v.clone())]))
            .collect();
        let vals = Expression::from_vec(values, ", ");

        expr_any!(
            "INSERT INTO {} ({}) VALUES ({})",
            (ident(&self.table)),
            (cols),
            (vals)
        )
    }
}

impl From<MysqlInsert> for Expr {
    fn from(insert: MysqlInsert) -> Self {
        insert.expr()
    }
}
