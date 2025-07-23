//! # SurrealDB Field Representation
//!
//! doc wip

use vantage_expressions::OwnedExpression;

use crate::{identifier::Identifier, operation::Expressive};

/// Represents a database field
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::select::field::Field;
///
/// // doc wip
/// let field = Field::new("user_name");
/// ```

#[derive(Debug, Clone, Hash)]
pub struct Field {
    field: String,
}

impl Field {
    /// Creates a new field
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `field` - doc wip
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl Into<OwnedExpression> for Field {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}

impl Expressive for Field {
    fn expr(&self) -> OwnedExpression {
        Identifier::new(self.field.clone()).into()
    }
}
