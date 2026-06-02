//! Comparison operators: ==, !=, <, >, <=, >=
//!
//! All operators accept Dynamic args and auto-convert
//! RhaiExpr, RhaiIdent, or scalar values.

use crate::AnySurrealType;
use crate::Expr;
use rhai::Dynamic;
use vantage_expressions::{Expressive, ExpressiveEnum, Expression};

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
    } else {
        Err(super::convert::rhai_err(format!(
            "operator: unsupported type '{}'",
            val.type_name()
        )))
    }
}

/// Build a binary comparison expression.
pub fn cmp_binary(op: &str, a: Expr, b: Expr) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("{{}} {} {{}}", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}
