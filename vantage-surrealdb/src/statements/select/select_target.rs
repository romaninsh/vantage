//! Targets of a SELECT's `FROM` clause.

use crate::surreal_expr;
use crate::{AnySurrealType, Expr, identifier::Identifier};
use vantage_core::IntoVec;
use vantage_expressions::{Expressive, ExpressiveEnum, traits::selectable::SourceRef};

/// One `FROM` target. A name becomes an escaped identifier; a nested
/// expression, such as a subquery, is wrapped in parentheses.
///
/// ```rust
/// use vantage_surrealdb::{statements::select::select_target::SelectTarget, surreal_expr};
///
/// let target = SelectTarget::new(surreal_expr!("users"));
/// ```

#[derive(Debug, Clone)]
pub struct SelectTarget {
    target: Expr,
}

impl SelectTarget {
    pub fn new(target: impl Into<Expr>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl From<SelectTarget> for Expr {
    fn from(val: SelectTarget) -> Self {
        val.target
    }
}

impl From<&str> for SelectTarget {
    fn from(s: &str) -> Self {
        Self::new(Identifier::new(s))
    }
}

impl From<String> for SelectTarget {
    fn from(s: String) -> Self {
        Self::new(Identifier::new(s))
    }
}

impl From<Identifier> for SelectTarget {
    fn from(id: Identifier) -> Self {
        Self::new(id)
    }
}

impl From<Expr> for SelectTarget {
    fn from(expr: Expr) -> Self {
        Self::new(expr)
    }
}

impl From<SourceRef<AnySurrealType>> for SelectTarget {
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

impl IntoVec<SelectTarget> for SelectTarget {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self]
    }
}

impl IntoVec<SelectTarget> for &str {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self.into()]
    }
}

impl IntoVec<SelectTarget> for String {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self.into()]
    }
}

impl IntoVec<SelectTarget> for Identifier {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self.into()]
    }
}

impl IntoVec<SelectTarget> for Expr {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self.into()]
    }
}

impl IntoVec<SelectTarget> for SourceRef<AnySurrealType> {
    fn into_vec(self) -> Vec<SelectTarget> {
        vec![self.into()]
    }
}
