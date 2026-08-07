//! Logical operators for SQL conditions: `or_()` and `and_()`, and the
//! [`ConditionGroup`] that they make.
//!
//! A group writes one set of brackets around the full chain. It writes
//! no brackets around each operand: `(a OR b OR c)`. A chain stays flat
//! while the operator does not change, and thus `a.or_(b).or_(c)` gives
//! `(a OR b OR c)`. To make a different group, nest the calls.
//! `a.or_(b.or_(c))` gives `(a OR (b OR c))`, because the inner group is
//! an operand and it writes its own brackets.
//!
//! The group writes the brackets. The statement renderer does not.
//! `WHERE` joins its conditions with a plain `AND`, and each group
//! arrives complete. This keeps the conditions of a table when a search
//! runs on top of them. The query says `role = 'admin' AND (a OR b)`. It
//! does not say `role = 'admin' AND a OR b`, which means
//! `(role = 'admin' AND a) OR b`, because `AND` binds more tightly than
//! `OR`.

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Conditions that one logical operator joins, written as a single
/// group in brackets. To make a group, call [`or_`] or [`and_`], or use
/// the `or_` and `and_` methods on a column or an identifier.
#[derive(Clone)]
pub struct ConditionGroup<T> {
    operator: &'static str,
    operands: Vec<Expression<T>>,
}

impl<T> std::fmt::Debug for ConditionGroup<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConditionGroup")
            .field("operator", &self.operator)
            .field("operands", &self.operands.len())
            .finish()
    }
}

impl<T: Clone> ConditionGroup<T> {
    pub(crate) fn new(operator: &'static str, operands: Vec<Expression<T>>) -> Self {
        Self { operator, operands }
    }

    /// Adds a condition with `OR`. If this group is an `OR` chain, the
    /// condition goes into the same group.
    ///
    /// This method is inherent. Rust selects it before the `or_` of the
    /// operation trait, and this keeps a chain flat.
    pub fn or_(self, other: impl Expressive<T>) -> Self {
        self.extend("OR", other.expr())
    }

    /// Adds a condition with `AND`. If this group is an `AND` chain, the
    /// condition goes into the same group.
    pub fn and_(self, other: impl Expressive<T>) -> Self {
        self.extend("AND", other.expr())
    }

    fn extend(mut self, operator: &'static str, other: Expression<T>) -> Self {
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

impl<T: Clone> Expressive<T> for ConditionGroup<T> {
    fn expr(&self) -> Expression<T> {
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

/// Joins two conditions with `OR`: `(lhs OR rhs)`.
///
/// ```ignore
/// use vantage_sql::primitives::*;
///
/// or_(ident("role").eq("admin"), ident("role").eq("superuser"))
/// // => ("role" = 'admin' OR "role" = 'superuser')
/// ```
pub fn or_<T: Clone>(lhs: impl Expressive<T>, rhs: impl Expressive<T>) -> ConditionGroup<T> {
    ConditionGroup::new("OR", vec![lhs.expr(), rhs.expr()])
}

/// Joins two conditions with `AND`: `(lhs AND rhs)`.
///
/// `with_condition()` joins its conditions with `AND` already. Use
/// `and_()` when you must make a group inside an `or_()`:
///
/// ```ignore
/// use vantage_sql::primitives::*;
///
/// // ((price > 100 AND in_stock = 1) OR featured = 1)
/// or_(
///     and_(ident("price").gt(100), ident("in_stock").eq(true)),
///     ident("featured").eq(true),
/// )
/// ```
pub fn and_<T: Clone>(lhs: impl Expressive<T>, rhs: impl Expressive<T>) -> ConditionGroup<T> {
    ConditionGroup::new("AND", vec![lhs.expr(), rhs.expr()])
}
