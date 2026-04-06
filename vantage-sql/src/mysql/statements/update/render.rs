use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::mysql::types::AnyMysqlType;

use super::{Expr, MysqlUpdate};

impl MysqlUpdate {
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

impl Expressive<AnyMysqlType> for MysqlUpdate {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return Expression::new("SELECT 1 WHERE FALSE", vec![]);
        }

        let set_parts: Vec<String> = self
            .fields
            .keys()
            .map(|k| format!("`{}` = {{}}", k))
            .collect();
        let template_base = format!("UPDATE `{}` SET {}", self.table, set_parts.join(", "));

        let mut params: Vec<ExpressiveEnum<AnyMysqlType>> = self
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

impl From<MysqlUpdate> for Expr {
    fn from(update: MysqlUpdate) -> Self {
        update.expr()
    }
}
