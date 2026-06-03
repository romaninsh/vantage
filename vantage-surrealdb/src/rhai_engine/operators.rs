//! Comparison operators: ==, !=, <, >, <=, >=
//!
//! All operators accept Dynamic args and auto-convert
//! RhaiExpr, RhaiIdent, or scalar values.

use crate::AnySurrealType;
use crate::Expr;
use rhai::Dynamic;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{RhaiExpr, RhaiIdent};

/// Convert a Dynamic value into an Expression.
/// Accepts RhaiExpr, RhaiIdent, i64, f64, bool, string.
pub fn to_expr(val: Dynamic) -> Result<Expr, Box<rhai::EvalAltResult>> {
    if let Some(e) = val.clone().try_cast::<RhaiExpr>() {
        Ok(e.0)
    } else if let Some(i) = val.clone().try_cast::<RhaiIdent>() {
        Ok(i.0.expr())
    } else if let Some(v) = val.clone().try_cast::<i64>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<f64>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<bool>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::from(v))],
        ))
    } else if let Some(v) = val.clone().try_cast::<rhai::ImmutableString>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::from(v.to_string()))],
        ))
    } else if let Some(v) = val.clone().try_cast::<String>() {
        Ok(Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::from(v))],
        ))
    } else if let Some(arr) = val.clone().try_cast::<rhai::Array>() {
        // a native `[…]` literal → SurrealQL array literal
        let items = arr
            .into_iter()
            .map(to_expr)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(crate::primitives::array_literal(items))
    } else if let Some(map) = val.clone().try_cast::<rhai::Map>() {
        // a native `#{…}` literal → SurrealQL object literal. Rhai's map is a
        // BTreeMap, so iteration is key-sorted (deterministic output).
        let mut entries = Vec::with_capacity(map.len());
        for (k, v) in map.into_iter() {
            entries.push((k.to_string(), to_expr(v)?));
        }
        Ok(crate::primitives::object_literal(entries))
    } else {
        Err(super::convert::rhai_err(format!(
            "operator: unsupported type '{}'",
            val.type_name()
        )))
    }
}

/// Lift a scalar value into an Expression (for arithmetic operands).
pub fn scalar(val: AnySurrealType) -> Expr {
    Expression::new("{}", vec![ExpressiveEnum::Scalar(val)])
}

/// Build a binary arithmetic expression, parenthesized: `(a op b)`. Used by the
/// `*`/`+`/`-`/`/` operators so closure bodies and ad-hoc maths render with
/// explicit precedence.
pub fn arith(op: &str, a: Expr, b: Expr) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("({{}} {} {{}})", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}

/// Build a binary comparison expression.
pub fn cmp_binary(op: &str, a: Expr, b: Expr) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("{{}} {} {{}}", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}
