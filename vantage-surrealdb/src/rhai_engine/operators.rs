//! Operator support: lowering a Rhai value to an expression, and the
//! comparison and arithmetic forms the registered operators build.

use crate::AnySurrealType;
use crate::Expr;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};
use vantage_rhai::rhai;
use vantage_rhai::rhai::Dynamic;

use super::{RhaiExpr, RhaiIdent};

/// Whether `val` is a query-builder operand (an expression or identifier), so
/// a comparison with it builds SurrealQL rather than comparing values.
pub(crate) fn is_builder(val: &Dynamic) -> bool {
    val.is::<RhaiExpr>() || val.is::<RhaiIdent>()
}

/// `a <op> b` over plain values, as Rhai compares them: numbers across int
/// and float, strings, bools, `()` (equal only to `()`), and arrays and maps
/// element by element. Values of different kinds are unequal; ordering them
/// is an error.
pub(crate) fn plain_cmp(
    op: &str,
    a: &Dynamic,
    b: &Dynamic,
) -> Result<bool, Box<rhai::EvalAltResult>> {
    use std::cmp::Ordering;
    if matches!(op, "==" | "!=") {
        let same = |x: &Dynamic, y: &Dynamic| matches!(plain_cmp("==", x, y), Ok(true));
        let equal = if let (Some(x), Some(y)) =
            (a.read_lock::<rhai::Array>(), b.read_lock::<rhai::Array>())
        {
            Some(x.len() == y.len() && x.iter().zip(y.iter()).all(|(x, y)| same(x, y)))
        } else if let (Some(x), Some(y)) = (a.read_lock::<rhai::Map>(), b.read_lock::<rhai::Map>())
        {
            Some(x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))))
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
