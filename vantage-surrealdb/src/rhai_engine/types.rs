//! Rhai wrapper types for the SurrealDB query-building DSL.

use crate::Expr;
use crate::identifier::Identifier;
use crate::primitives::Case;
use crate::statements::SurrealSelect;
use std::fmt;

/// Identifier wrapper (table/column name)
#[derive(Clone, Debug)]
pub struct RhaiIdent(pub Identifier);

impl RhaiIdent {
    pub fn into_inner(self) -> Identifier {
        self.0
    }
}

/// Expression wrapper
pub struct RhaiExpr(pub Expr);

impl Clone for RhaiExpr {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for RhaiExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RhaiExpr").field(&self.0).finish()
    }
}

impl RhaiExpr {
    pub fn into_inner(self) -> Expr {
        self.0
    }
}

/// CASE / IF builder wrapper (`case_when().when().else_().expr()`)
#[derive(Clone, Debug)]
pub struct RhaiCase(pub Case);

/// Select query builder wrapper
#[derive(Clone, Debug)]
pub struct RhaiSelect {
    pub inner: SurrealSelect,
}

impl RhaiSelect {
    pub fn new() -> Self {
        Self {
            inner: SurrealSelect::new(),
        }
    }

    pub fn into_inner(self) -> SurrealSelect {
        self.inner
    }

    /// Preview the query as a string (for testing)
    pub fn preview(&self) -> String {
        self.inner.preview()
    }
}

impl Default for RhaiSelect {
    fn default() -> Self {
        Self::new()
    }
}
