//! SurrealDB identifiers (table and field names), escaped when rendered.

use crate::Expr;
use crate::surreal_expr;
use vantage_expressions::{Expressive, ExpressiveOr};
use vantage_table::column::core::{Column, ColumnType};

/// A table or field name. A plain name renders bare; a reserved keyword or
/// any other name that needs it renders ⟨quoted⟩.
///
/// ```rust
/// use vantage_surrealdb::identifier::Identifier;
///
/// let id = Identifier::new("user_name");
/// let escaped = Identifier::new("SELECT"); // Reserved keyword
/// ```
#[derive(Debug, Clone)]
pub struct Identifier {
    identifier: String,
}

impl Identifier {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }

    /// `self.other`, each segment escaped on its own.
    pub fn dot(self, other: impl Into<String>) -> crate::Expr {
        surreal_expr!("{}.{}", (self), (Identifier::new(other.into())))
    }
}

impl From<Identifier> for crate::Expr {
    fn from(val: Identifier) -> Self {
        val.expr()
    }
}

impl Expressive<crate::AnySurrealType> for Identifier {
    fn expr(&self) -> crate::Expr {
        use vantage_expressions::Expression;
        // `surreal-client` owns the escaping rules, so they can't drift.
        Expression::new(surreal_client::escape_identifier(&self.identifier), vec![])
    }
}

/// The `$parent` subquery parameter.
pub struct Parent {}
impl Parent {
    /// `$parent.field`: `$parent` is a built-in parameter, emitted verbatim;
    /// the field is escaped.
    pub fn dot(field: impl Into<String>) -> crate::Expr {
        crate::surreal_expr!("$parent.{}", (Identifier::new(field.into())))
    }
}

// A string given where a field is expected names a column; any other
// expression passes through.

impl ExpressiveOr<crate::AnySurrealType, Identifier> for &str {
    fn field_expr(&self) -> Expr {
        Identifier::new(*self).expr()
    }
}

impl ExpressiveOr<crate::AnySurrealType, Identifier> for String {
    fn field_expr(&self) -> Expr {
        Identifier::new(self.as_str()).expr()
    }
}

impl ExpressiveOr<crate::AnySurrealType, Identifier> for Identifier {
    fn field_expr(&self) -> Expr {
        Expressive::expr(self)
    }
}

impl ExpressiveOr<crate::AnySurrealType, Identifier> for Expr {
    fn field_expr(&self) -> Expr {
        self.clone()
    }
}

impl ExpressiveOr<crate::AnySurrealType, Identifier> for crate::field::Field {
    fn field_expr(&self) -> Expr {
        Expressive::expr(self)
    }
}

impl<T: ColumnType> ExpressiveOr<crate::AnySurrealType, Identifier> for Column<T> {
    fn field_expr(&self) -> Expr {
        Identifier::new(self.name()).expr()
    }
}
