//! Fields of a SELECT's projection.

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

/// One projected expression, optionally `AS alias` and optionally `VALUE`.
///
/// ```rust
/// use vantage_surrealdb::{select::select_field::SelectField, surreal_expr};
///
/// let field = SelectField::new(surreal_expr!("name"));
/// let aliased = SelectField::new(surreal_expr!("count()")).with_alias("total".to_string());
/// ```

#[derive(Debug, Clone)]
pub struct SelectField {
    expression: Expr,
    alias: Option<String>,
    is_value: bool,
}

impl SelectField {
    pub fn new(expression: impl Expressive<AnySurrealType>) -> Self {
        Self {
            expression: expression.expr(),
            alias: None,
            is_value: false,
        }
    }

    /// Render as `expr AS alias`, the alias escaped.
    pub fn with_alias(mut self, alias: String) -> Self {
        self.alias = Some(alias);
        self
    }

    /// Render with a leading `VALUE`.
    pub fn as_value(mut self) -> Self {
        self.is_value = true;
        self
    }
}

impl From<SelectField> for Expr {
    fn from(val: SelectField) -> Self {
        match (&val.alias, val.is_value) {
            (Some(alias), true) => {
                surreal_expr!("VALUE {} AS {}", (val.expression), (Identifier::new(alias)))
            }
            (Some(alias), false) => {
                surreal_expr!("{} AS {}", (val.expression), (Identifier::new(alias)))
            }
            (None, true) => surreal_expr!("VALUE {}", (val.expression)),
            (None, false) => val.expression,
        }
    }
}
