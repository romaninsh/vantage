use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{Expr, SqliteSelect};

impl SqliteSelect {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            from: Vec::new(),
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
        }
    }

    pub fn from(mut self, table: &str) -> Self {
        self.from
            .push(Expression::new(format!("\"{}\"", table), vec![]));
        self
    }

    pub fn field(mut self, name: &str) -> Self {
        self.fields
            .push(Expression::new(format!("\"{}\"", name), vec![]));
        self
    }

    pub fn with_expression(mut self, expression: Expr, alias: Option<String>) -> Self {
        let field = match alias {
            Some(a) => Expression::new(
                format!("{{}} AS \"{}\"", a),
                vec![ExpressiveEnum::Nested(expression)],
            ),
            None => expression,
        };
        self.fields.push(field);
        self
    }

    pub fn with_where(mut self, condition: impl Expressive<JsonValue>) -> Self {
        self.where_conditions.push(condition.expr());
        self
    }

    pub fn with_order_by(mut self, field: &str, ascending: bool) -> Self {
        self.order_by.push((
            Expression::new(format!("\"{}\"", field), vec![]),
            ascending,
        ));
        self
    }

    pub fn with_order_by_expr(mut self, expression: Expr, ascending: bool) -> Self {
        self.order_by.push((expression, ascending));
        self
    }

    pub fn with_group_by(mut self, field: &str) -> Self {
        self.group_by
            .push(Expression::new(format!("\"{}\"", field), vec![]));
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_skip(mut self, skip: i64) -> Self {
        self.skip = Some(skip);
        self
    }

    pub fn with_distinct(mut self) -> Self {
        self.distinct = true;
        self
    }
}
