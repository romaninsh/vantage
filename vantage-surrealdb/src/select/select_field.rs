//! # SurrealDB Select Fields
//!
//! doc wip

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

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
    expression: Expr,
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
    pub fn new(expression: impl Expressive<AnySurrealType>) -> Self {
        Self {
            expression: expression.expr(),
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
