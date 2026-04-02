//! SurrealDB operations for expressions
//!
//! This module provides operations that extend expressions with SurrealDB-specific functionality.

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

// Use the pre-defined ExprArguments enum from lib.rs
use crate::SurrealExprArgs;

// ExprArguments now automatically gets Expressive blanket impl from fn_args! macro

/// Extension trait to add reference traversal methods to expressions
pub trait RefOperation: Expressive<AnySurrealType> {
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
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr;

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
    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr;
    fn eq(&self, other: impl Into<SurrealExprArgs>) -> Expr;
    fn sub(&self, other: impl Into<SurrealExprArgs>) -> Expr;
    fn contains(&self, other: impl Into<SurrealExprArgs>) -> Expr;
    fn in_(&self, other: impl Into<SurrealExprArgs>) -> Expr;
}

// Default implementations for RefOperation
impl<T> RefOperation for T
where
    T: Expressive<AnySurrealType>,
{
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr {
        surreal_expr!(
            "{}->{}->{}",
            self,
            Identifier::new(reference),
            Identifier::new(table)
        )
    }

    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr {
        surreal_expr!(
            "{}<-{}<-{}",
            self,
            Identifier::new(reference),
            Identifier::new(table)
        )
    }

    fn eq(&self, other: impl Into<SurrealExprArgs>) -> Expr {
        surreal_expr!("{} = {}", self, other.into())
    }

    fn sub(&self, other: impl Into<SurrealExprArgs>) -> Expr {
        surreal_expr!("{} - {}", self, other.into())
    }

    fn contains(&self, other: impl Into<SurrealExprArgs>) -> Expr {
        surreal_expr!("{} CONTAINS {}", self, other.into())
    }

    fn in_(&self, other: impl Into<SurrealExprArgs>) -> Expr {
        surreal_expr!("{} IN {}", self, other.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_operations() {
        // Test basic right and left references
        let expr = surreal_expr!("bakery");
        assert_eq!(
            expr.rref("owns", "product").preview(),
            "bakery->owns->product"
        );
        assert_eq!(
            expr.lref("owns", "product").preview(),
            "bakery<-owns<-product"
        );

        // Test with complex identifiers
        let complex_expr = surreal_expr!("bakery:hill_valley");
        assert_eq!(
            complex_expr.rref("owns", "product").preview(),
            "bakery:hill_valley->owns->product"
        );

        let id_expr = surreal_expr!("product:1");
        assert_eq!(
            id_expr.lref("owns", "bakery").preview(),
            "product:1<-owns<-bakery"
        );
    }

    #[test]
    fn test_reference_chaining() {
        // Test right reference chaining
        let user_expr = surreal_expr!("user");
        assert_eq!(
            user_expr
                .rref("owns", "company")
                .rref("has", "employees")
                .preview(),
            "user->owns->company->has->employees"
        );

        // Test mixed chaining
        let product_expr = surreal_expr!("product:1");
        assert_eq!(
            product_expr
                .lref("owns", "bakery")
                .rref("located_in", "city")
                .preview(),
            "product:1<-owns<-bakery->located_in->city"
        );
    }

    #[test]
    fn test_comprehensive_api() {
        use crate::thing::Thing;

        // Test different expression types
        let bakery_expr = surreal_expr!("bakery");
        assert_eq!(
            bakery_expr.rref("owns", "product").preview(),
            "bakery->owns->product"
        );

        let bakery_thing = Thing::new("bakery", "hill_valley");
        assert_eq!(
            bakery_thing.rref("owns", "product").preview(),
            "bakery:hill_valley->owns->product"
        );

        // Test complex chaining across different types
        let complex_traversal = surreal_expr!("user")
            .rref("owns", "company")
            .lref("employs", "employee")
            .rref("lives_in", "city");
        assert_eq!(
            complex_traversal.preview(),
            "user->owns->company<-employs<-employee->lives_in->city"
        );
    }

    #[test]
    fn test_comparison_operations() {
        let field = surreal_expr!("age");

        // Test eq with scalar value
        let eq_scalar = field.eq(25i32);
        assert_eq!(eq_scalar.preview(), "age = 25");

        // Test eq with expression
        let other_field = surreal_expr!("max_age");
        let eq_expr = field.eq(other_field);
        assert_eq!(eq_expr.preview(), "age = max_age");

        // Test sub operation
        let sub_result = field.sub(10i32);
        assert_eq!(sub_result.preview(), "age - 10");

        // Test contains operation
        let tags_field = surreal_expr!("tags");
        let contains_result = tags_field.contains("bakery".to_string());
        assert_eq!(contains_result.preview(), r#"tags CONTAINS "bakery""#);

        // Test in_ operation with expression
        let status_field = surreal_expr!("status");
        let values_expr = surreal_expr!(r#"["active", "pending"]"#);
        let in_result = status_field.in_(values_expr);
        assert_eq!(in_result.preview(), r#"status IN ["active", "pending"]"#);
    }
}
