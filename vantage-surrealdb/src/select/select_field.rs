//! # SurrealDB Select Fields
//!
//! doc wip

use vantage_expressions::{OwnedExpression, expr};

use crate::identifier::Identifier;

/// Represents a field in a SELECT clause
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::expr;
/// use vantage_surrealdb::select::select_field::SelectField;
///
/// // doc wip
/// let field = SelectField::new(expr!("name"));
/// let aliased = SelectField::new(expr!("count()")).with_alias("total".to_string());
/// ```

#[derive(Debug, Clone)]
pub struct SelectField {
    expression: OwnedExpression,
    alias: Option<String>,
    is_value: bool, // For VALUE clause
}

impl SelectField {
    /// Creates a new select field
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `expression` - doc wip
    pub fn new(expression: impl Into<OwnedExpression>) -> Self {
        Self {
            expression: expression.into(),
            alias: None,
            is_value: false,
        }
    }

    /// Adds an alias to the field
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `alias` - doc wip
    pub fn with_alias(mut self, alias: String) -> Self {
        self.alias = Some(alias);
        self
    }

    /// Marks field as a VALUE expression
    ///
    /// doc wip
    pub fn as_value(mut self) -> Self {
        self.is_value = true;
        self
    }
}

impl Into<OwnedExpression> for SelectField {
    fn into(self) -> OwnedExpression {
        match (&self.alias, self.is_value) {
            (Some(alias), true) => expr!("VALUE {} AS {}", self.expression, Identifier::new(alias)),
            (Some(alias), false) => expr!("{} AS {}", self.expression, Identifier::new(alias)),
            (None, true) => expr!("VALUE {}", self.expression),
            (None, false) => self.expression,
        }
    }
}
