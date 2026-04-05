use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{Expr, SqliteInsert};

impl SqliteInsert {
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<JsonValue> for SqliteInsert {
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

        let params: Vec<ExpressiveEnum<JsonValue>> = self
            .fields
            .values()
            .map(|v| ExpressiveEnum::Scalar(v.clone()))
            .collect();

        Expression::new(template, params)
    }
}

impl From<SqliteInsert> for Expr {
    fn from(insert: SqliteInsert) -> Self {
        insert.expr()
    }
}
