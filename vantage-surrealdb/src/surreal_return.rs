//! # Surreal Return implements RETURN query
//!
//! doc wip

use std::{marker::PhantomData, ops::Deref, path::Path, thread::ScopedJoinHandle};

use vantage_expressions::{OwnedExpression, expr, result};

use crate::{operation::Expressive, protocol::SurrealQueriable};

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
pub struct SurrealReturn<T = result::Single> {
    expr: OwnedExpression,
    _phantom: PhantomData<T>,
}

impl SurrealReturn {
    /// Calculate sum of expression
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(expr: OwnedExpression) -> Self {
        Self {
            expr: expr!("RETURN {}", expr),
            _phantom: PhantomData,
        }
    }
}

impl Deref for SurrealReturn {
    type Target = OwnedExpression;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}
impl SurrealQueriable for SurrealReturn {}
impl Expressive for SurrealReturn {
    fn expr(&self) -> OwnedExpression {
        self.expr.clone()
    }
}

impl Into<OwnedExpression> for SurrealReturn {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}
