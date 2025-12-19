//! # Surreal Return implements RETURN query
//!
//! doc wip

use std::{marker::PhantomData, ops::Deref};

use vantage_expressions::{Expressive, result};

use crate::{AnySurrealType, Expr, surreal_expr};

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
    expr: Expr,
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
    pub fn new(expr: Expr) -> Self {
        Self {
            expr: surreal_expr!("RETURN {}", (expr)),
            _phantom: PhantomData,
        }
    }
}

// impl SurrealReturn<result::Single> {
//     pub async fn get(&self, db: &SurrealDB) -> Value {
//         db.execute(&self.expr()).await
//     }
// }

impl Deref for SurrealReturn {
    type Target = Expr;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}
// impl SurrealQueriable for SurrealReturn {}
impl Expressive<AnySurrealType> for SurrealReturn {
    fn expr(&self) -> Expr {
        self.expr.clone()
    }
}
