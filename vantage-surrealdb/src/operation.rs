//! SurrealDB operations for expressions
//!
//! This module provides operations that extend expressions with SurrealDB-specific functionality.

use vantage_expressions::{Expr, OwnedExpression, expr};

use crate::identifier::Identifier;

/// Trait for types that can be converted to OwnedExpression
pub trait Expressive: Into<OwnedExpression> {
    /// Convert to OwnedExpression
    fn expr(&self) -> OwnedExpression;
}

/// Extension trait to add reference traversal methods to expressions
pub trait RefOperation: Expressive {
    /// Creates a right reference traversal expression in the format: self->ref->table
    ///
    /// # Arguments
    ///
    /// * `reference` - The reference/edge name to traverse
    /// * `table` - The target table name
    ///
    /// # Returns
    ///
    /// An expression that renders as "self->reference->table"
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> OwnedExpression;

    /// Creates a left reference traversal expression in the format: self<-ref<-table
    ///
    /// # Arguments
    ///
    /// * `reference` - The reference/edge name to traverse
    /// * `table` - The target table name
    ///
    /// # Returns
    ///
    /// An expression that renders as "self<-reference<-table"
    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> OwnedExpression;
    fn alias(&self, alias: impl Into<String>) -> OwnedExpression;
    fn eq(&self, other: impl Into<Expr>) -> OwnedExpression;
    fn sub(&self, other: impl Into<Expr>) -> OwnedExpression;
    fn contains(&self, other: impl Into<Expr>) -> OwnedExpression;
    fn in_(&self, other: impl Into<Expr>) -> OwnedExpression;
}

// Default implementations for RefOperation
impl<T> RefOperation for T
where
    T: Expressive,
{
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> OwnedExpression {
        expr!(
            "{}->{}->{}",
            self.expr(),
            OwnedExpression::new(reference.into(), vec![]),
            OwnedExpression::new(table.into(), vec![])
        )
    }

    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> OwnedExpression {
        expr!(
            "{}<-{}<-{}",
            self.expr(),
            OwnedExpression::new(reference.into(), vec![]),
            OwnedExpression::new(table.into(), vec![])
        )
    }

    fn alias(&self, alias: impl Into<String>) -> OwnedExpression {
        expr!("{} AS {}", self.expr(), Identifier::new(alias.into()))
    }

    fn eq(&self, other: impl Into<Expr>) -> OwnedExpression {
        expr!("{} = {}", self.expr(), other.into())
    }

    fn sub(&self, other: impl Into<Expr>) -> OwnedExpression {
        expr!("{} - {}", self.expr(), other.into())
    }

    fn contains(&self, other: impl Into<Expr>) -> OwnedExpression {
        expr!("{} CONTAINS {}", self.expr(), other.into())
    }

    fn in_(&self, other: impl Into<Expr>) -> OwnedExpression {
        expr!("{} IN {}", self.expr(), other.into())
    }
}

impl Expressive for OwnedExpression {
    fn expr(&self) -> OwnedExpression {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_expressions::expr;

    #[test]
    fn test_rref_operation_basic() {
        let expr = expr!("bakery");
        let trav = expr.rref("owns", "product");

        let result = trav.preview();
        assert_eq!(result, "bakery->owns->product");
    }

    #[test]
    fn test_lref_operation_basic() {
        let expr = expr!("bakery");
        let trav = expr.lref("owns", "product");

        let result = trav.preview();
        assert_eq!(result, "bakery<-owns<-product");
    }

    #[test]
    fn test_rref_operation_with_complex_expr() {
        let expr = expr!("bakery:hill_valley");
        let trav = expr.rref("owns", "product");

        let result = trav.preview();
        assert_eq!(result, "bakery:hill_valley->owns->product");
    }

    #[test]
    fn test_lref_operation_with_complex_expr() {
        let expr = expr!("product:1");
        let trav = expr.lref("owns", "bakery");

        let result = trav.preview();
        assert_eq!(result, "product:1<-owns<-bakery");
    }

    #[test]
    fn test_rref_operation_chaining() {
        let expr = expr!("user");
        let trav = expr.rref("owns", "company").rref("has", "employees");

        let result = trav.preview();
        assert_eq!(result, "user->owns->company->has->employees");
    }

    #[test]
    fn test_mixed_ref_operation_chaining() {
        let expr = expr!("product:1");
        let trav = expr.lref("owns", "bakery").rref("located_in", "city");

        let result = trav.preview();
        assert_eq!(result, "product:1<-owns<-bakery->located_in->city");
    }

    #[test]
    fn test_comprehensive_api_usage() {
        use crate::thing::Thing;

        // Test with expressions
        let bakery_expr = expr!("bakery");
        let product_traversal = bakery_expr.rref("owns", "product");
        assert_eq!(product_traversal.preview(), "bakery->owns->product");

        // Test with Thing
        let bakery_thing = Thing::new("bakery", "hill_valley");
        let product_from_thing = bakery_thing.rref("owns", "product");
        assert_eq!(
            product_from_thing.preview(),
            "bakery:hill_valley->owns->product"
        );

        // Test left reference
        let product_expr = expr!("product:1");
        let bakery_from_product = product_expr.lref("owns", "bakery");
        assert_eq!(bakery_from_product.preview(), "product:1<-owns<-bakery");

        // Test chaining
        let complex_traversal = expr!("user")
            .rref("owns", "company")
            .lref("employs", "employee")
            .rref("lives_in", "city");
        assert_eq!(
            complex_traversal.preview(),
            "user->owns->company<-employs<-employee->lives_in->city"
        );
    }
}
