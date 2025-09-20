//! # Surreal Return implements RETURN query
//!
//! doc wip

use std::{marker::PhantomData, ops::Deref};

use serde_json::Value;
use vantage_expressions::{DataSource, Expression, expr, result};

use crate::{SurrealDB, operation::Expressive, protocol::SurrealQueriable};

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
    expr: Expression,
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
    pub fn new(expr: Expression) -> Self {
        Self {
            expr: expr!("RETURN {}", expr),
            _phantom: PhantomData,
        }
    }
}

impl SurrealReturn<result::Single> {
    pub async fn get(&self, db: &SurrealDB) -> Value {
        db.execute(&self.expr()).await
    }
}

impl Deref for SurrealReturn {
    type Target = Expression;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}
impl SurrealQueriable for SurrealReturn {}
impl Expressive for SurrealReturn {
    fn expr(&self) -> Expression {
        self.expr.clone()
    }
}

impl Into<Expression> for SurrealReturn {
    fn into(self) -> Expression {
        self.expr()
    }
}
