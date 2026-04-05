use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{Expr, SqliteUpdate};

impl SqliteUpdate {
    fn render_where(&self) -> Option<Expr> {
        if self.conditions.is_empty() {
            return None;
        }
        Some(Expression::from_vec(self.conditions.clone(), " AND "))
    }

    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<JsonValue> for SqliteUpdate {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return Expression::new(format!("UPDATE \"{}\"", self.table), vec![]);
        }

        let set_parts: Vec<String> = self
            .fields
            .keys()
            .map(|k| format!("\"{}\" = {{}}", k))
            .collect();
        let template_base = format!("UPDATE \"{}\" SET {}", self.table, set_parts.join(", "));

        let mut params: Vec<ExpressiveEnum<JsonValue>> = self
            .fields
            .values()
            .map(|v| ExpressiveEnum::Scalar(v.clone()))
            .collect();

        let template = match self.render_where() {
            Some(cond) => {
                params.push(ExpressiveEnum::Nested(cond));
                format!("{} WHERE {{}}", template_base)
            }
            None => template_base,
        };

        Expression::new(template, params)
    }
}

impl From<SqliteUpdate> for Expr {
    fn from(update: SqliteUpdate) -> Self {
        update.expr()
    }
}
