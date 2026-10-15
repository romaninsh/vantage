//! Operator support: lowering a Rhai value to an expression, and the
//! comparison and arithmetic forms the registered operators build.

use crate::AnySurrealType;
use crate::Expr;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};
use vantage_rhai::rhai;
use vantage_rhai::rhai::Dynamic;

use super::{RhaiExpr, RhaiIdent};

/// A Rhai value as an expression: an expression or identifier as it is, a
/// scalar bound as a parameter, a `[…]` array or `#{…}` map as a literal.
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
        let items = arr
            .into_iter()
            .map(to_expr)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(crate::primitives::array_literal(items))
    } else if let Some(map) = val.clone().try_cast::<rhai::Map>() {
        // Rhai's map is a BTreeMap, so the keys render sorted.
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

/// `(a op b)`, parenthesized so arithmetic renders with explicit precedence.
pub fn arith(op: &str, a: Expr, b: Expr) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("({{}} {} {{}})", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}

/// `a op b`, unparenthesized.
pub fn cmp_binary(op: &str, a: Expr, b: Expr) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("{{}} {} {{}}", op),
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    ))
}
