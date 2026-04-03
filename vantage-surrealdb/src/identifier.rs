//! # SurrealDB Identifiers
//!
//! doc wip

use crate::Expr;
use crate::surreal_expr;
use vantage_expressions::{Expressive, ExpressiveOr};
use vantage_table::column::core::{Column, ColumnType};

/// SurrealDB identifier with automatic escaping
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::identifier::Identifier;
///
/// // doc wip
/// let id = Identifier::new("user_name");
/// let escaped = Identifier::new("SELECT"); // Reserved keyword
/// ```
#[derive(Debug, Clone)]
pub struct Identifier {
    identifier: String,
}

impl Identifier {
    /// Creates a new identifier
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }

    pub fn dot(self, other: impl Into<String>) -> crate::Expr {
        surreal_expr!("{}.{}", (self), (Identifier::new(other.into())))
    }

    /// Determines if identifier needs escaping
    ///
    /// doc wip
    fn needs_escaping(&self) -> bool {
        let reserved_keywords = [
            "DEFINE", "CREATE", "SELECT", "UPDATE", "DELETE", "FROM", "RETURN", "WHERE", "SET",
            "ONLY", "TABLE",
        ];

        let upper_identifier = self.identifier.to_uppercase();

        // Check if it contains spaces or is a reserved keyword
        self.identifier.contains(' ') || reserved_keywords.contains(&upper_identifier.as_str())
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
        if self.needs_escaping() {
            Expression::new(format!("⟨{}⟩", self.identifier), vec![])
        } else {
            Expression::new(self.identifier.clone(), vec![])
        }
    }
}

pub struct Parent {}
impl Parent {
    pub fn identifier() -> Identifier {
        Identifier::new("$parent")
    }
}

// ExpressiveOr<AnySurrealType, Identifier> impls
// Strings go through Identifier (unquoted column names),
// everything else passes through via Expressive.

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

impl ExpressiveOr<crate::AnySurrealType, Identifier> for crate::statements::select::field::Field {
    fn field_expr(&self) -> Expr {
        Expressive::expr(self)
    }
}

impl<T: ColumnType> ExpressiveOr<crate::AnySurrealType, Identifier> for Column<T> {
    fn field_expr(&self) -> Expr {
        Identifier::new(self.name()).expr()
    }
}
