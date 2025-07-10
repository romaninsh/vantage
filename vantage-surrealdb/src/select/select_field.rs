use vantage_expressions::{OwnedExpression, expr};

use crate::identifier::Identifier;

#[derive(Debug, Clone)]
pub struct SelectField {
    expression: OwnedExpression,
    alias: Option<String>,
    is_value: bool, // For VALUE clause
}

impl SelectField {
    pub fn new(expression: impl Into<OwnedExpression>) -> Self {
        Self {
            expression: expression.into(),
            alias: None,
            is_value: false,
        }
    }

    pub fn with_alias(mut self, alias: String) -> Self {
        self.alias = Some(alias);
        self
    }

    pub fn as_value(mut self) -> Self {
        self.is_value = true;
        self
    }
}

impl Into<OwnedExpression> for SelectField {
    fn into(self) -> OwnedExpression {
        let base_expr = self.expression.preview();

        match (&self.alias, self.is_value) {
            (Some(alias), true) => expr!(
                "VALUE {} AS {}",
                Identifier::new(base_expr),
                Identifier::new(alias)
            ),
            (Some(alias), false) => expr!(
                "{} AS {}",
                Identifier::new(base_expr),
                Identifier::new(alias)
            ),
            (None, true) => expr!("VALUE {}", Identifier::new(base_expr)),
            (None, false) => self.expression,
        }
    }
}
