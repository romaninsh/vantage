//! # SurrealDB Field Representation
//!
//! doc wip

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier};

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

    pub fn dot(&self, field: impl Into<String>) -> Expr {
        Identifier::new(self.field.clone()).dot(field.into())
    }
}

impl Expressive<AnySurrealType> for Field {
    fn expr(&self) -> Expr {
        Identifier::new(self.field.clone()).into()
    }
}
