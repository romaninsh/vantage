//! # SurrealDB Query Targets
//!
//! doc wip

use crate::surreal_expr;
use crate::{AnySurrealType, Expr, identifier::Identifier};
use vantage_core::IntoVec;
use vantage_expressions::{Expressive, ExpressiveEnum, traits::selectable::SourceRef};

/// Represents a target in a FROM clause
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::{select::select_target::Target, surreal_expr};
///
/// let target = Target::new(surreal_expr!("users"));
/// ```

#[derive(Debug, Clone)]
pub struct Target {
    target: Expr,
}

impl Target {
    /// Creates a new query target
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `target` - doc wip
    pub fn new(target: impl Into<Expr>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl From<Target> for Expr {
    fn from(val: Target) -> Self {
        val.target
    }
}

impl From<&str> for Target {
    fn from(s: &str) -> Self {
        Self::new(Identifier::new(s))
    }
}

impl From<String> for Target {
    fn from(s: String) -> Self {
        Self::new(Identifier::new(s))
    }
}

impl From<Identifier> for Target {
    fn from(id: Identifier) -> Self {
        Self::new(id)
    }
}

impl From<Expr> for Target {
    fn from(expr: Expr) -> Self {
        Self::new(expr)
    }
}

impl From<SourceRef<AnySurrealType>> for Target {
    fn from(source_ref: SourceRef<AnySurrealType>) -> Self {
        let expr = match source_ref.into_expressive_enum() {
            ExpressiveEnum::Scalar(s) => {
                let source: String = s
                    .try_get::<String>()
                    .unwrap_or_else(|| panic!("Source must be a string, found {:?}", s));
                Identifier::new(source).expr()
            }
            ExpressiveEnum::Nested(expr) => surreal_expr!("({})", (expr)),
            ExpressiveEnum::Deferred(_) => {
                panic!("Cannot use deferred as select source")
            }
        };
        Self::new(expr)
    }
}

// IntoVec<Target> impls for single items

impl IntoVec<Target> for Target {
    fn into_vec(self) -> Vec<Target> {
        vec![self]
    }
}

impl IntoVec<Target> for &str {
    fn into_vec(self) -> Vec<Target> {
        vec![self.into()]
    }
}

impl IntoVec<Target> for String {
    fn into_vec(self) -> Vec<Target> {
        vec![self.into()]
    }
}

impl IntoVec<Target> for Identifier {
    fn into_vec(self) -> Vec<Target> {
        vec![self.into()]
    }
}

impl IntoVec<Target> for Expr {
    fn into_vec(self) -> Vec<Target> {
        vec![self.into()]
    }
}

impl IntoVec<Target> for SourceRef<AnySurrealType> {
    fn into_vec(self) -> Vec<Target> {
        vec![self.into()]
    }
}
