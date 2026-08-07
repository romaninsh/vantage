//! SurrealDB-specific operations for expressions.
//!
//! - `SurrealOperation` — common comparison methods (eq, ne, gt, gte, lt, lte,
//!   in_) that give an `Expression<AnySurrealType>`. It also has `or_` and
//!   `and_`, which join conditions into a [`ConditionGroup`].
//! - `RefOperation` — SurrealDB-specific graph traversal (rref/lref),
//!   subtraction, CONTAINS, and parenthesis-free IN.

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

/// Conditions that one logical operator joins. The group writes one set
/// of brackets around the full chain. It writes no brackets around each
/// operand: `(a OR b OR c)`.
///
/// A chain stays flat while the operator does not change. `a.or_(b).or_(c)`
/// gives `(a OR b OR c)`. To make a different group, nest the calls.
/// `a.or_(b.or_(c))` gives `(a OR (b OR c))`, because the inner group is
/// an operand and it writes its own brackets.
///
/// The group writes the brackets. The statement renderer does not.
/// `WHERE` joins its conditions with a plain `AND`, and each group
/// arrives complete. This keeps the conditions of a table when a search
/// runs on top of them. The query says `status = 'x' AND (a OR b)`. It
/// does not say `status = 'x' AND a OR b`, which means
/// `(status = 'x' AND a) OR b`, because `AND` binds more tightly
/// than `OR`.
#[derive(Debug, Clone)]
pub struct ConditionGroup {
    operator: &'static str,
    operands: Vec<Expr>,
}

impl ConditionGroup {
    pub(crate) fn new(operator: &'static str, operands: Vec<Expr>) -> Self {
        Self { operator, operands }
    }

    /// Adds a condition with `OR`. If this group is an `OR` chain, the
    /// condition goes into the same group.
    ///
    /// This method is inherent. Rust selects it before the blanket
    /// [`SurrealOperation::or_`], and this keeps a chain flat.
    pub fn or_(self, other: impl Expressive<AnySurrealType>) -> Self {
        self.extend("OR", other.expr())
    }

    /// Adds a condition with `AND`. If this group is an `AND` chain, the
    /// condition goes into the same group.
    pub fn and_(self, other: impl Expressive<AnySurrealType>) -> Self {
        self.extend("AND", other.expr())
    }

    fn extend(mut self, operator: &'static str, other: Expr) -> Self {
        if self.operator == operator {
            self.operands.push(other);
            self
        } else {
            // The operator changes. The chain to this point becomes one
            // operand of the new group. It keeps its brackets and its
            // meaning.
            Self::new(operator, vec![self.expr(), other])
        }
    }
}

impl Expressive<AnySurrealType> for ConditionGroup {
    fn expr(&self) -> Expr {
        let separator = format!(" {} ", self.operator);
        let template = std::iter::repeat_n("{}", self.operands.len())
            .collect::<Vec<_>>()
            .join(&separator);
        Expression::new(
            format!("({template})"),
            self.operands
                .iter()
                .cloned()
                .map(ExpressiveEnum::Nested)
                .collect(),
        )
    }
}

/// Standard comparison operations for SurrealDB expressions.
///
/// Blanket-implemented for all `Expressive<AnySurrealType>`.
pub trait SurrealOperation: Expressive<AnySurrealType> {
    /// `(self OR other)` — joins two conditions into a
    /// [`ConditionGroup`].
    ///
    /// Use this method to write alternatives. Do not write `"a OR b"` as
    /// text. The group writes its own brackets, and it keeps its meaning
    /// next to the other conditions. Text has no brackets, and `AND`
    /// binds more tightly than `OR`. Thus `status = 'x' AND a OR b`
    /// means `(status = 'x' AND a) OR b`.
    fn or_(&self, other: impl Expressive<AnySurrealType>) -> ConditionGroup
    where
        Self: Sized,
    {
        ConditionGroup::new("OR", vec![self.expr(), other.expr()])
    }

    /// `(self AND other)` — joins two conditions into a
    /// [`ConditionGroup`].
    ///
    /// A table joins its conditions with `AND` already. Use this method
    /// when you must make a group inside an [`Self::or_`].
    fn and_(&self, other: impl Expressive<AnySurrealType>) -> ConditionGroup
    where
        Self: Sized,
    {
        ConditionGroup::new("AND", vec![self.expr(), other.expr()])
    }

    /// `field = value`
    fn eq(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field != value`
    fn ne(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} != {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field > value`
    fn gt(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} > {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field >= value`
    fn gte(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} >= {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field < value`
    fn lt(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} < {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field <= value`
    fn lte(&self, value: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} <= {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field IN (values_expression)`
    fn in_(&self, values: impl Expressive<AnySurrealType>) -> Expr
    where
        Self: Sized,
    {
        Expression::new(
            "{} IN ({})",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(values.expr()),
            ],
        )
    }
}

impl<S: Expressive<AnySurrealType>> SurrealOperation for S {}

/// SurrealDB-specific operations: graph traversal, CONTAINS, subtraction,
/// and parenthesis-free IN.
///
/// For standard comparisons (eq, ne, gt, gte, lt, lte, in_), use `SurrealOperation`.
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
        let field = surreal_expr!("age");

        // eq comes from SurrealOperation
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
