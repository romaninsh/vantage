//! # SurrealDB Identifiers
//!
//! doc wip

use vantage_expressions::{IntoExpressive, OwnedExpression, expr};

use crate::operation::Expressive;

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

    pub fn dot(self, other: impl Into<String>) -> OwnedExpression {
        expr!("{}.{}", self, Identifier::new(other.into()))
    }

    /// Determines if identifier needs escaping
    ///
    /// doc wip
    fn needs_escaping(&self) -> bool {
        let reserved_keywords = [
            "DEFINE", "CREATE", "SELECT", "UPDATE", "DELETE", "FROM", "WHERE", "SET", "ONLY",
            "TABLE",
        ];

        let upper_identifier = self.identifier.to_uppercase();

        // Check if it contains spaces or is a reserved keyword
        self.identifier.contains(' ') || reserved_keywords.contains(&upper_identifier.as_str())
    }
}

impl Into<OwnedExpression> for Identifier {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}

impl From<Identifier> for IntoExpressive<OwnedExpression> {
    fn from(id: Identifier) -> Self {
        IntoExpressive::nested(id.into())
    }
}

impl Expressive for Identifier {
    fn expr(&self) -> OwnedExpression {
        if self.needs_escaping() {
            expr!(format!("⟨{}⟩", self.identifier))
        } else {
            expr!(self.identifier.clone())
        }
    }
}

pub struct Parent {}
impl Parent {
    pub fn new() -> Identifier {
        Identifier::new("$parent")
    }
}
