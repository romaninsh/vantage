//! Comparison operators: ==, !=, <, >, <=, >=
//!
//! All operators accept Dynamic args and auto-convert
//! RhaiExpr, RhaiIdent, or scalar values.

use rhai::Dynamic;
use vantage_expressions::{Expressive, ExpressiveEnum, Expression};
use crate::primitives::identifier::Identifier;

use super::{RhaiExpr, RhaiIdent};

/// Convert a Dynamic value into an Expression<V>.
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
        Ok(Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(v))]))
    } else if let Some(v) = val.clone().try_cast::<f64>() {
        Ok(Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(v))]))
    } else if let Some(v) = val.clone().try_cast::<bool>() {
        Ok(Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(v))]))
    } else if let Some(v) = val.clone().try_cast::<rhai::ImmutableString>() {
        Ok(Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(v.to_string()))]))
    } else if let Some(v) = val.clone().try_cast::<String>() {
        Ok(Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(v))]))
    } else {
        Err(super::convert::rhai_err(format!(
            "operator: unsupported type '{}'", val.type_name()
        )))
    }
}

/// Build a binary comparison expression.
pub fn cmp_binary<V: Clone>(
    op: &str,
    a: Expression<V>,
    b: Expression<V>,
) -> RhaiExpr<V> {
    RhaiExpr(Expression::new(
        &format!("{{}} {} {{}}", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}

// ── Macro: register all comparison operators ───────────────────────────
//
// Uses Dynamic args so we only need 1 function per operator (not 20+).

#[macro_export]
macro_rules! register_operators {
    ($engine:expr, value: $V:ty) => {{
        // Each closure takes (Dynamic, Dynamic), converts both sides,
        // and produces a comparison RhaiExpr.

        macro_rules! reg_op {
            ($rhai_op:expr, $sql_op:expr, $name:ident) => {
                fn $name(a: rhai::Dynamic, b: rhai::Dynamic)
                    -> Result<$crate::rhai_engine::RhaiExpr<$V>, Box<rhai::EvalAltResult>>
                {
                    let la = $crate::rhai_engine::operators::to_expr::<$V>(a)?;
                    let rb = $crate::rhai_engine::operators::to_expr::<$V>(b)?;
                    Ok($crate::rhai_engine::operators::cmp_binary($sql_op, la, rb))
                }
                $engine.register_fn($rhai_op, $name);
            };
        }

        reg_op!("==", "=",   cmp_eq);
        reg_op!("!=", "!=",  cmp_ne);
        reg_op!("<",  "<",   cmp_lt);
        reg_op!(">",  ">",   cmp_gt);
        reg_op!("<=", "<=",  cmp_le);
        reg_op!(">=", ">=",  cmp_ge);
    }};
}
