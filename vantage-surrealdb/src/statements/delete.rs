//! SurrealDB `DELETE` statement builder.
//!
//! Builds parameterized `DELETE target [WHERE ...]` expressions.
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealDelete, thing::Thing, surreal_expr};
//!
//! // Delete a single record
//! let del = SurrealDelete::new(Thing::new("users", "john"));
//!
//! // Delete all records in a table
//! let del = SurrealDelete::table("sessions");
//!
//! // Conditional delete
//! let del = SurrealDelete::table("logs")
//!     .with_condition(surreal_expr!("level = {}", "debug"))
//!     .with_condition(surreal_expr!("age > {}", 30i64));
//!
//! // Execute
//! db.execute(&del.expr()).await?;
//! ```

use vantage_expressions::Expressive;

use crate::Expr;
use crate::identifier::Identifier;
use crate::types::AnySurrealType;

/// Builder for SurrealDB `DELETE` statements.
///
/// Produces `DELETE target` or `DELETE target WHERE ...`.
/// Multiple conditions are combined with AND.
pub struct SurrealDelete {
    /// Target expression (table name, `Thing`, or arbitrary expression).
    pub target: Expr,
    /// Optional WHERE conditions (combined with AND).
    pub conditions: Vec<Expr>,
}

impl SurrealDelete {
    /// Delete all records from a table: `DELETE tablename`
    pub fn table(table: &str) -> Self {
        Self {
            target: Identifier::new(table).expr(),
            conditions: Vec::new(),
        }
    }

    /// Delete a specific target (e.g. a [`Thing`](crate::thing::Thing) record ID).
    pub fn new(target: impl Expressive<AnySurrealType>) -> Self {
        Self {
            target: target.expr(),
            conditions: Vec::new(),
        }
    }

    /// Add a WHERE condition. Multiple conditions are combined with AND.
    pub fn with_condition(mut self, condition: impl Expressive<AnySurrealType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }

    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySurrealType> for SurrealDelete {
    fn expr(&self) -> Expr {
        if self.conditions.is_empty() {
            return crate::surreal_expr!("DELETE {}", (self.target));
        }

        let combined = self
            .conditions
            .iter()
            .cloned()
            .reduce(|a, b| crate::surreal_expr!("{} AND {}", (a), (b)))
            .unwrap();

        crate::surreal_expr!("DELETE {} WHERE {}", (self.target), (combined))
    }
}

impl From<SurrealDelete> for Expr {
    fn from(delete: SurrealDelete) -> Self {
        delete.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thing::Thing;

    #[test]
    fn test_delete_table() {
        let del = SurrealDelete::table("users");
        assert_eq!(del.preview(), "DELETE users");
    }

    #[test]
    fn test_delete_record() {
        let del = SurrealDelete::new(Thing::new("users", "john"));
        assert_eq!(del.preview(), "DELETE users:john");
    }

    #[test]
    fn test_delete_with_condition() {
        let del = SurrealDelete::table("users")
            .with_condition(crate::surreal_expr!("active = {}", false));
        assert_eq!(del.preview(), "DELETE users WHERE active = false");
    }

    #[test]
    fn test_delete_with_multiple_conditions() {
        let del = SurrealDelete::table("logs")
            .with_condition(crate::surreal_expr!("level = {}", "debug"))
            .with_condition(crate::surreal_expr!("age > {}", 30i64));
        assert_eq!(
            del.preview(),
            "DELETE logs WHERE level = \"debug\" AND age > 30"
        );
    }

    #[test]
    fn test_delete_identifier_escaping() {
        let del = SurrealDelete::table("SELECT");
        assert_eq!(del.preview(), "DELETE ⟨SELECT⟩");
    }

    #[test]
    fn test_delete_produces_parameterized_expression() {
        let del =
            SurrealDelete::table("users").with_condition(crate::surreal_expr!("score < {}", 10i64));
        let expr = del.expr();
        assert!(expr.template.contains("{}"));
    }
}
