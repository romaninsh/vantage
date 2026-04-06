//! SurrealDB-specific operations for expressions.
//!
//! Common comparison methods (eq, ne, gt, gte, lt, lte, in_) are provided by the
//! generic `Operation<T>` trait from vantage-table, which has a blanket impl for
//! all `Expressive<T>` types. This module adds SurrealDB-specific operations:
//! graph traversal (rref/lref), subtraction, CONTAINS, and a SurrealDB-flavored
//! IN (without parentheses).

use vantage_expressions::Expressive;

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

/// SurrealDB-specific operations: graph traversal, CONTAINS, subtraction,
/// and parenthesis-free IN.
///
/// For standard comparisons (eq, ne, gt, gte, lt, lte), use `Operation<T>`
/// from `vantage_table::operation` — it's blanket-implemented for all
/// `Expressive<T>` types.
pub trait RefOperation: Expressive<AnySurrealType> {
    /// Right-side graph traversal: `self->reference->table`
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr;
    /// Left-side graph traversal: `self<-reference<-table`
    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr;
    /// Subtraction: `self - other`
    fn sub(&self, other: impl Expressive<AnySurrealType>) -> Expr;
    /// SurrealDB CONTAINS operator: `self CONTAINS other`
    fn contains_(&self, other: impl Expressive<AnySurrealType>) -> Expr;
    /// SurrealDB IN without parentheses: `self IN other`
    ///
    /// SurrealDB uses `value IN array` syntax where the right side can be
    /// a graph traversal or array literal — no parentheses needed.
    /// SQL backends should use `Operation::in_()` which adds parens for subqueries.
    fn surreal_in(&self, other: impl Expressive<AnySurrealType>) -> Expr;
}

impl<T> RefOperation for T
where
    T: Expressive<AnySurrealType>,
{
    fn rref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr {
        surreal_expr!(
            "{}->{}->{}",
            (self),
            (Identifier::new(reference)),
            (Identifier::new(table))
        )
    }

    fn lref(&self, reference: impl Into<String>, table: impl Into<String>) -> Expr {
        surreal_expr!(
            "{}<-{}<-{}",
            (self),
            (Identifier::new(reference)),
            (Identifier::new(table))
        )
    }

    fn sub(&self, other: impl Expressive<AnySurrealType>) -> Expr {
        surreal_expr!("{} - {}", (self), (other))
    }

    fn contains_(&self, other: impl Expressive<AnySurrealType>) -> Expr {
        surreal_expr!("{} CONTAINS {}", (self), (other))
    }

    fn surreal_in(&self, other: impl Expressive<AnySurrealType>) -> Expr {
        surreal_expr!("{} IN {}", (self), (other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_operations() {
        let expr = surreal_expr!("bakery");
        assert_eq!(
            expr.rref("owns", "product").preview(),
            "bakery->owns->product"
        );
        assert_eq!(
            expr.lref("owns", "product").preview(),
            "bakery<-owns<-product"
        );

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
        let user_expr = surreal_expr!("user");
        assert_eq!(
            user_expr
                .rref("owns", "company")
                .rref("has", "employees")
                .preview(),
            "user->owns->company->has->employees"
        );

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
        use vantage_table::operation::Operation;

        let field = surreal_expr!("age");

        // eq comes from generic Operation<T>
        let eq_scalar = field.eq(25i64);
        assert_eq!(eq_scalar.preview(), "age = 25");

        // sub is SurrealDB-specific
        let sub_result = field.sub(10i64);
        assert_eq!(sub_result.preview(), "age - 10");

        let tags_field = surreal_expr!("tags");
        let contains_result = tags_field.contains_("bakery".to_string());
        assert_eq!(contains_result.preview(), r#"tags CONTAINS "bakery""#);

        // surreal_in — paren-free SurrealDB syntax
        let status_field = surreal_expr!("status");
        let values_expr = surreal_expr!(r#"["active", "pending"]"#);
        let in_result = status_field.surreal_in(values_expr);
        assert_eq!(in_result.preview(), r#"status IN ["active", "pending"]"#);
    }
}
