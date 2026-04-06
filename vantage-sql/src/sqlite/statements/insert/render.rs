use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::primitives::identifier::ident;
use crate::sqlite::types::AnySqliteType;

use super::{Expr, SqliteInsert};

impl SqliteInsert {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySqliteType> for SqliteInsert {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return expr_any!("INSERT INTO {} DEFAULT VALUES", (ident(&self.table)));
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

impl From<SqliteInsert> for Expr {
    fn from(insert: SqliteInsert) -> Self {
        insert.expr()
    }
}
