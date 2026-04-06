use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::mysql::types::AnyMysqlType;

use super::{Expr, MysqlInsert};

impl MysqlInsert {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyMysqlType> for MysqlInsert {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return Expression::new(
                format!("INSERT INTO `{}` () VALUES ()", self.table),
                vec![],
            );
        }

        let columns: Vec<String> = self.fields.keys().map(|k| format!("`{}`", k)).collect();
        let placeholders: Vec<&str> = (0..self.fields.len()).map(|_| "{}").collect();

        let template = format!(
            "INSERT INTO `{}` ({}) VALUES ({})",
            self.table,
            columns.join(", "),
            placeholders.join(", ")
        );

        let params: Vec<ExpressiveEnum<AnyMysqlType>> = self
            .fields
            .values()
            .map(|v| ExpressiveEnum::Scalar(v.clone()))
            .collect();

        Expression::new(template, params)
    }
}

impl From<MysqlInsert> for Expr {
    fn from(insert: MysqlInsert) -> Self {
        insert.expr()
    }
}
