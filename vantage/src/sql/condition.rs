use std::sync::Arc;

use serde_json::Value;

use crate::expr;
use crate::prelude::Column;
use crate::sql::expression::{Expression, ExpressionArc};
use crate::sql::Chunk;

use super::table::column::SqlColumn;

#[derive(Debug, Clone)]
enum ConditionOperand {
    Column(Arc<Column>),
    Expression(Box<Expression>),
    Condition(Box<Condition>),
    Value(Value),
}

#[derive(Debug, Clone)]
pub struct Condition {
    field: ConditionOperand,
    operation: String,
    value: Arc<Box<dyn Chunk>>,
}

#[allow(dead_code)]
impl Condition {
    pub fn from_field(
        field: Arc<Column>,
        operation: &str,
        value: Arc<Box<dyn Chunk>>,
    ) -> Condition {
        Condition {
            field: ConditionOperand::Column(field),
            operation: operation.to_string(),
            value,
        }
    }
    pub fn from_expression(
        expression: Expression,
        operation: &str,
        value: Arc<Box<dyn Chunk>>,
    ) -> Condition {
        Condition {
            field: ConditionOperand::Expression(Box::new(expression)),
            operation: operation.to_string(),
            value,
        }
    }
    pub fn from_condition(
        condition: Condition,
        operation: &str,
        value: Arc<Box<dyn Chunk>>,
    ) -> Condition {
        Condition {
            field: ConditionOperand::Condition(Box::new(condition)),
            operation: operation.to_string(),
            value,
        }
    }

    pub fn set_table_alias(&mut self, alias: &str) {
        match &mut self.field {
            ConditionOperand::Column(field) => {
                let mut f = field.as_ref().clone();
                f.set_table_alias(alias.to_string());
                *field = Arc::new(f);
            }
            ConditionOperand::Condition(condition) => condition.set_table_alias(alias),
            _ => {}
        }
    }

    pub fn from_value(operand: Value, operation: &str, value: Arc<Box<dyn Chunk>>) -> Condition {
        Condition {
            field: ConditionOperand::Value(operand),
            operation: operation.to_string(),
            value,
        }
    }

    fn render_operand(&self) -> Expression {
        match self.field.clone() {
            ConditionOperand::Column(field) => field.render_chunk(),
            ConditionOperand::Expression(expression) => expression.render_chunk(),
            ConditionOperand::Condition(condition) => condition.render_chunk(),
            ConditionOperand::Value(value) => expr!("{}", value.clone()).render_chunk(),
        }
    }

    pub fn and(self, other: Condition) -> Condition {
        Condition::from_condition(self, "AND", Arc::new(Box::new(other)))
    }

    pub fn or(self, other: Condition) -> Condition {
        Condition::from_condition(self, "OR", Arc::new(Box::new(other)))
    }
}

impl Chunk for Condition {
    fn render_chunk(&self) -> Expression {
        ExpressionArc::new(
            format!("({{}} {} {{}})", self.operation),
            vec![
                Arc::new(Box::new(self.render_operand())),
                self.value.clone(),
            ],
        )
        .render_chunk()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition() {
        let field = Arc::new(Column::new("id".to_string(), None));

        let condition = Condition::from_field(field, "=", Arc::new(Box::new("1".to_string())));
        let (sql, params) = condition.render_chunk().split();

        assert_eq!(sql, "(id = {})");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], "1");
    }

    #[test]
    fn test_condition_expression() {
        let expression = expr!("1 + 1");

        let condition =
            Condition::from_expression(expression, "=", Arc::new(Box::new("1".to_string())));
        let (sql, params) = condition.render_chunk().split();

        assert_eq!(sql, "(1 + 1 = {})");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], "1");
    }

    #[test]
    fn test_and() {
        let f_married = Arc::new(Column::new("married".to_string(), None));
        let f_divorced = Arc::new(Column::new("divorced".to_string(), None));

        let condition =
            Condition::from_field(f_married, "=", Arc::new(Box::new("yes".to_string()))).and(
                Condition::from_field(f_divorced, "=", Arc::new(Box::new("yes".to_string()))),
            );

        let (sql, params) = condition.render_chunk().split();

        assert_eq!(sql, "((married = {}) AND (divorced = {}))");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], "yes");
        assert_eq!(params[1], "yes");
    }
}
