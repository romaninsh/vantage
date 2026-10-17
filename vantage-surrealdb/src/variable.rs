//! SurrealDB `$`-variables.

use vantage_expressions::{Expression, Expressive};

use crate::{AnySurrealType, Expr};

/// A SurrealDB `$`-variable reference, lowering to `$name`.
///
/// Covers the built-in scope variables (`$parent`, `$this`, `$value`,
/// `$before`/`$after`, `$session`, `$auth`, …) and any `LET`-bound name.
/// Compose a field access with the expression-layer `.` (e.g. via
/// [`crate::primitives::field`]) to get `$parent.id`.
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::Expressive;
/// use vantage_surrealdb::variable::Variable;
///
/// assert_eq!(Variable::new("parent").expr().preview(), "$parent");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable {
    name: String,
}

impl Variable {
    /// Creates a new variable from a bare name (no leading `$`).
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Expressive<AnySurrealType> for Variable {
    fn expr(&self) -> Expr {
        // The name carries no `{}`, so it renders as a literal `$name`.
        Expression::new(format!("${}", self.name), vec![])
    }
}

impl From<Variable> for Expr {
    fn from(val: Variable) -> Self {
        val.expr()
    }
}
