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
}

impl From<Identifier> for crate::Expr {
    fn from(val: Identifier) -> Self {
        val.expr()
    }
}

impl Expressive<crate::AnySurrealType> for Identifier {
    fn expr(&self) -> crate::Expr {
        use vantage_expressions::Expression;
        // Single escaping authority lives in `surreal-client` so the rules
        // can't drift between the two query builders.
        Expression::new(surreal_client::escape_identifier(&self.identifier), vec![])
    }
}

pub struct Parent {}
impl Parent {
    /// `$parent` is a SurrealQL built-in subquery parameter — not a user
    /// identifier — so it must be emitted verbatim, not through `escape_identifier`.
    pub fn dot(field: impl Into<String>) -> crate::Expr {
        crate::surreal_expr!("$parent.{}", (Identifier::new(field.into())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_close_bracket_cannot_break_out_of_quoting() {
        // `\⟩` is an invalid escape inside ⟨…⟩, so a `⟩` must be emitted as the
        // `\u{27E9}` unicode escape — the only form the SurrealDB lexer accepts.
        let expr = Identifier::new("a⟩b").expr();
        assert_eq!(expr.preview(), "⟨a\\u{27E9}b⟩");
    }

    #[test]
    fn crafted_backslash_bracket_cannot_break_out_of_quoting() {
        // A raw `\⟩` would collapse (via `\\`) and let the `⟩` close the
        // identifier early, injecting whatever follows. The backslash must be
        // doubled so the whole sequence stays one identifier.
        let expr = Identifier::new("a\\⟩: 1 }; RETURN 999; x").expr();
        assert_eq!(expr.preview(), "⟨a\\\\\\u{27E9}: 1 }; RETURN 999; x⟩");
    }

    #[test]
    fn reserved_keyword_is_escaped() {
        let expr = Identifier::new("SELECT").expr();
        assert_eq!(expr.preview(), "⟨SELECT⟩");
    }

    #[test]
    fn plain_identifier_is_unquoted() {
        let expr = Identifier::new("user_name").expr();
        assert_eq!(expr.preview(), "user_name");
    }
}
