use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::postgres::types::AnyPostgresType;

use super::{Expr, PostgresInsert};

impl PostgresInsert {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnyPostgresType> for PostgresInsert {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return Expression::new(
                format!("INSERT INTO \"{}\" DEFAULT VALUES", self.table),
                vec![],
            );
        }

        let columns: Vec<String> = self.fields.keys().map(|k| format!("\"{}\"", k)).collect();
        let placeholders: Vec<&str> = (0..self.fields.len()).map(|_| "{}").collect();

        let template = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            self.table,
            columns.join(", "),
            placeholders.join(", ")
        );

        let params: Vec<ExpressiveEnum<AnyPostgresType>> = self
            .fields
            .values()
            .map(|v| ExpressiveEnum::Scalar(v.clone()))
            .collect();

        Expression::new(template, params)
    }
}

impl From<PostgresInsert> for Expr {
    fn from(insert: PostgresInsert) -> Self {
        insert.expr()
    }
}
