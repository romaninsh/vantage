//! Comparison operators: ==, !=, <, >, <=, >=
//!
//! All operators accept Dynamic args and auto-convert
//! RhaiExpr, RhaiIdent, or scalar values.

use crate::primitives::identifier::Identifier;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};
use vantage_rhai::rhai;
use vantage_rhai::rhai::Dynamic;

use super::{RhaiExpr, RhaiIdent};

/// Convert a Dynamic value into an `Expression<V>`.
/// Accepts RhaiExpr, RhaiIdent, i64, f64, bool, string.
pub fn to_expr<V>(val: Dynamic) -> Result<Expression<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    if let Some(e) = val.clone().try_cast::<RhaiExpr<V>>() {
        Ok(e.0)
    } else if let Some(i) = val.clone().try_cast::<RhaiIdent>() {
        Ok(i.0.expr())
    } else if let Some(v) = val.clone().try_cast::<i64>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(V::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<f64>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(V::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<bool>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(V::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<rhai::ImmutableString>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(V::from(v.to_string()))],
        ))
    } else if let Some(v) = val.clone().try_cast::<String>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(V::from(v))],
        ))
    } else {
        Err(super::convert::rhai_err(format!(
            "operator: unsupported type '{}'",
            val.type_name()
        )))
    }
}

/// Whether `val` is a query-builder operand (an expression or identifier), so
/// a comparison with it builds SQL rather than comparing values.
pub fn is_builder<V: Clone + Send + Sync + 'static>(val: &Dynamic) -> bool {
    val.is::<RhaiExpr<V>>() || val.is::<RhaiIdent>()
}

/// `a <op> b` over plain values, as Rhai compares them: numbers across int
/// and float, strings, bools, and `()` (equal only to `()`). Values of
/// different kinds are unequal; ordering them is an error.
pub fn plain_cmp(op: &str, a: &Dynamic, b: &Dynamic) -> Result<bool, Box<rhai::EvalAltResult>> {
    use std::cmp::Ordering;
    // Arrays and maps are equal when their elements are.
    if matches!(op, "==" | "!=") {
        let same = |x: &Dynamic, y: &Dynamic| matches!(plain_cmp("==", x, y), Ok(true));
        let equal = if let (Some(x), Some(y)) = (
            a.read_lock::<rhai::Array>(),
            b.read_lock::<rhai::Array>(),
        ) {
            Some(x.len() == y.len() && x.iter().zip(y.iter()).all(|(x, y)| same(x, y)))
        } else if let (Some(x), Some(y)) =
            (a.read_lock::<rhai::Map>(), b.read_lock::<rhai::Map>())
        {
            Some(
                x.len() == y.len()
                    && x.iter()
                        .all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
            )
        } else {
            None
        };
        if let Some(equal) = equal {
            return Ok(equal == (op == "=="));
        }
    }
    let number = |v: &Dynamic| {
        v.as_int()
            .map(|i| i as f64)
            .ok()
            .or_else(|| v.as_float().ok())
    };
    let ordering: Option<Ordering> = if let (Some(x), Some(y)) = (number(a), number(b)) {
        x.partial_cmp(&y)
    } else if let (Ok(x), Ok(y)) = (a.as_immutable_string_ref(), b.as_immutable_string_ref()) {
        Some(x.as_str().cmp(y.as_str()))
    } else if let (Ok(x), Ok(y)) = (a.as_bool(), b.as_bool()) {
        Some(x.cmp(&y))
    } else if let (Ok(x), Ok(y)) = (a.as_char(), b.as_char()) {
        Some(x.cmp(&y))
    } else if a.is_unit() && b.is_unit() {
        Some(Ordering::Equal)
    } else {
        None
    };
    match (op, ordering) {
        ("==", o) => Ok(o == Some(Ordering::Equal)),
        ("!=", o) => Ok(o != Some(Ordering::Equal)),
        ("<", Some(o)) => Ok(o == Ordering::Less),
        (">", Some(o)) => Ok(o == Ordering::Greater),
        ("<=", Some(o)) => Ok(o != Ordering::Greater),
        (">=", Some(o)) => Ok(o != Ordering::Less),
        _ => Err(super::convert::rhai_err(format!(
            "can't compare '{}' {op} '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Build a binary comparison expression.
pub fn cmp_binary<V: Clone>(op: &str, a: Expression<V>, b: Expression<V>) -> RhaiExpr<V> {
    RhaiExpr(Expression::new(
        format!("{{}} {} {{}}", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}

// Dynamic args need one function per operator, not one per operand type pair.
#[macro_export]
macro_rules! register_operators {
    ($engine:expr, value: $V:ty) => {{
        macro_rules! reg_op {
            ($rhai_op:expr, $sql_op:expr, $name:ident) => {
                // Builder operands make SQL; plain values (a rule hook's
                // `"dev" == env`, `list.contains(x)`) compare to a bool.
                fn $name(
                    a: rhai::Dynamic,
                    b: rhai::Dynamic,
                ) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                    use $crate::rhai_engine::operators::{is_builder, plain_cmp};
                    if !is_builder::<$V>(&a) && !is_builder::<$V>(&b) {
                        return plain_cmp($rhai_op, &a, &b).map(rhai::Dynamic::from);
                    }
                    let la = $crate::rhai_engine::operators::to_expr::<$V>(a)?;
                    let rb = $crate::rhai_engine::operators::to_expr::<$V>(b)?;
                    Ok(rhai::Dynamic::from(
                        $crate::rhai_engine::operators::cmp_binary($sql_op, la, rb),
                    ))
                }
                $engine.register_fn($rhai_op, $name);
            };
        }

        reg_op!("==", "=", cmp_eq);
        reg_op!("!=", "!=", cmp_ne);
        reg_op!("<", "<", cmp_lt);
        reg_op!(">", ">", cmp_gt);
        reg_op!("<=", "<=", cmp_le);
        reg_op!(">=", ">=", cmp_ge);

        // `a.or_(b)` / `a.and_(b)`: a bracketed group, flat while the
        // operator doesn't change (see `primitives::logical`).
        macro_rules! reg_group {
            ($rhai_fn:expr, $name:ident, $group:path) => {
                fn $name(
                    a: rhai::Dynamic,
                    b: rhai::Dynamic,
                ) -> Result<$crate::rhai_engine::RhaiExpr<$V>, Box<rhai::EvalAltResult>> {
                    let la = $crate::rhai_engine::operators::to_expr::<$V>(a)?;
                    let rb = $crate::rhai_engine::operators::to_expr::<$V>(b)?;
                    Ok($crate::rhai_engine::RhaiExpr(
                        $crate::vantage_expressions::Expressive::expr(&$group(la, rb)),
                    ))
                }
                $engine.register_fn($rhai_fn, $name);
            };
        }

        reg_group!("or_", group_or, $crate::primitives::logical::or_);
        reg_group!("and_", group_and, $crate::primitives::logical::and_);
    }};
}
