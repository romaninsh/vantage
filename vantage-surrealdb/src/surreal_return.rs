//! `RETURN expr` statements.

use std::{marker::PhantomData, ops::Deref};

use vantage_expressions::{Expressive, result};

use crate::{AnySurrealType, Expr, surreal_expr};

/// A `RETURN expr` statement. Derefs to the rendered expression.
#[derive(Debug, Clone)]
pub struct SurrealReturn<T = result::Single> {
    expr: Expr,
    _phantom: PhantomData<T>,
}

impl SurrealReturn {
    pub fn new(expr: Expr) -> Self {
        Self {
            expr: surreal_expr!("RETURN {}", (expr)),
            _phantom: PhantomData,
        }
    }
}

impl Deref for SurrealReturn {
    type Target = Expr;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}

impl Expressive<AnySurrealType> for SurrealReturn {
    fn expr(&self) -> Expr {
        self.expr.clone()
    }
}
