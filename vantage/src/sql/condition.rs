use serde_json::Value;
use std::sync::Arc;
use std::sync::RwLock;

use crate::expr;
use crate::sql::Chunk;
use crate::sql::expression::{Expression, ExpressionArc};

use super::table::Column;

#[derive(Clone)]
enum ConditionOperand {
    Column(Arc<Column>),
    Expression(Box<Expression>),
    Condition(Box<Condition>),
    Value(Value),
}

impl std::fmt::Debug for ConditionOperand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
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
        column: Arc<Column>,
        operation: &str,
        value: Arc<Box<dyn Chunk>>,
    ) -> Condition {
        Condition {
            field: ConditionOperand::Column(column),
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

    // pub fn set_table_alias(&mut self, alias: &str) {
    //     match &mut self.field {
    //         ConditionOperand::Column(field) => {
    //             field.set_table_alias(alias.to_string());
    //         }
    //         ConditionOperand::Condition(condition) => condition.set_table_alias(alias),
    //         _ => {}
    //     }
    // }

    pub fn from_value(operand: Value, operation: &str, value: Arc<Box<dyn Chunk>>) -> Condition {
        Condition {
            field: ConditionOperand::Value(operand),
            operation: operation.to_string(),
            value,
        }
    }

    fn render_operand(&self) -> Expression {
        match self.field.clone() {
            ConditionOperand::Column(field) => expr!(field.name_with_table()),
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
    use serde_json::json;

    use crate::{
        mocks::MockDataSource,
        prelude::{AnyTable, SqlTable},
        sql::Table,
    };

    use super::*;

    #[test]
    fn test_condition() {
        let ds = MockDataSource::new(&json!([]));

        let table = Table::new("test", ds).with_column("id");

        let condition = Condition::from_field(
            table.get_column_box("id").unwrap(),
            "=",
            Arc::new(Box::new("1".to_string())),
        );
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
        let ds = MockDataSource::new(&json!([]));

        let table = Table::new("test", ds)
            .with_column("married")
            .with_column("divorced");

        let condition = Condition::from_field(
            table.get_column_box("married").unwrap(),
            "=",
            Arc::new(Box::new("yes".to_string())),
        )
        .and(Condition::from_field(
            table.get_column_box("divorced").unwrap(),
            "=",
            Arc::new(Box::new("yes".to_string())),
        ));

        let (sql, params) = condition.render_chunk().split();

        assert_eq!(sql, "((married = {}) AND (divorced = {}))");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], "yes");
        assert_eq!(params[1], "yes");
    }
}
