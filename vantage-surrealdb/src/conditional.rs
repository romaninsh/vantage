use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone)]
pub struct Conditional {
    condition: OwnedExpression,
    then_expr: OwnedExpression,
    else_expr: OwnedExpression,
}

impl Conditional {
    pub fn new(
        condition: impl Into<OwnedExpression>,
        then_expr: impl Into<OwnedExpression>,
        else_expr: impl Into<OwnedExpression>,
    ) -> Self {
        Self {
            condition: condition.into(),
            then_expr: then_expr.into(),
            else_expr: else_expr.into(),
        }
    }
}

impl Into<OwnedExpression> for Conditional {
    fn into(self) -> OwnedExpression {
        expr!(
            "IF ({}) THEN ({}) ELSE ({}) END",
            self.condition,
            self.then_expr,
            self.else_expr
        )
    }
}
