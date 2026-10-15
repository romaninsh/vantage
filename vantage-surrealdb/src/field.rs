//! A named field, for conditions and projections.

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier};

/// A field by name, rendered as an escaped [`Identifier`].
///
/// ```rust
/// use vantage_surrealdb::field::Field;
///
/// let field = Field::new("user_name");
/// ```

#[derive(Debug, Clone, Hash)]
pub struct Field {
    field: String,
}

impl Field {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }

    /// `field.other`, each segment escaped on its own.
    pub fn dot(&self, field: impl Into<String>) -> Expr {
        Identifier::new(self.field.clone()).dot(field.into())
    }
}

impl Expressive<AnySurrealType> for Field {
    fn expr(&self) -> Expr {
        Identifier::new(self.field.clone()).into()
    }
}
