//! Condition helpers — build eq-conditions for `Table<RestApi, _>` and
//! peel them apart at the `list_values` boundary into URL query params.
//!
//! Why not the standard `Operation::eq` from `vantage-table`? That trait
//! is blanket-implemented for all `Expressive<T>`, but the value-side
//! `Expressive<ciborium::Value>` impls for primitive types live in
//! foreign crates — we can't add them ourselves. So we ship a focused
//! `eq_condition` helper that produces the same expression shape
//! directly.

use ciborium::Value as CborValue;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;

/// Build an `eq` condition `field = value` for `Table<RestApi, _>`.
///
/// `value` is accepted as anything that converts into `CborValue` —
/// most scalars (`i64`, `f64`, `bool`, `&str`, `String`) implement
/// this directly through ciborium's `From` impls, so the call site
/// stays readable: `eq_condition("userId", 1i64)`.
pub fn eq_condition(field: &str, value: impl Into<CborValue>) -> Expression<CborValue> {
    Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new(field.to_string(), vec![])),
            ExpressiveEnum::Nested(Expression::new(
                "{}",
                vec![ExpressiveEnum::Scalar(value.into())],
            )),
        ],
    )
}

/// Try to peel an `eq`-shaped condition into `(field_name, value_string)`
/// suitable for a URL query parameter. Returns `None` for anything
/// that doesn't match — non-eq operators, compound values, nested
/// column-on-column comparisons, deferred values, etc.
pub(crate) fn condition_to_query_param(cond: &Expression<CborValue>) -> Option<(String, String)> {
    if cond.template != "{} = {}" || cond.parameters.len() != 2 {
        return None;
    }

    let field = match &cond.parameters[0] {
        ExpressiveEnum::Nested(e) if e.parameters.is_empty() => e.template.clone(),
        _ => return None,
    };

    let value = match &cond.parameters[1] {
        ExpressiveEnum::Nested(e) if e.template == "{}" && e.parameters.len() == 1 => {
            match &e.parameters[0] {
                ExpressiveEnum::Scalar(v) => cbor_to_query_string(v)?,
                _ => return None,
            }
        }
        ExpressiveEnum::Scalar(v) => cbor_to_query_string(v)?,
        _ => return None,
    };

    Some((field, value))
}

/// Render a scalar CBOR value as the string form expected in a URL
/// query parameter. Compound values (arrays, maps) have no single-key
/// representation, so they fall through as `None` and the condition
/// is dropped from the query string.
pub(crate) fn cbor_to_query_string(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Bool(b) => Some(b.to_string()),
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Float(f) => Some(f.to_string()),
        CborValue::Text(s) => Some(s.clone()),
        CborValue::Null | CborValue::Array(_) | CborValue::Map(_) | CborValue::Bytes(_) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_condition_round_trips_int() {
        let cond = eq_condition("userId", 1i64);
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("userId".into(), "1".into()));
    }

    #[test]
    fn eq_condition_round_trips_string() {
        let cond = eq_condition("name", "Alice");
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("name".into(), "Alice".into()));
    }

    #[test]
    fn eq_condition_round_trips_bool() {
        let cond = eq_condition("completed", true);
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("completed".into(), "true".into()));
    }

    #[test]
    fn raw_expression_with_unknown_template_returns_none() {
        let cond = Expression::<CborValue>::new("CUSTOM SQL", vec![]);
        assert!(condition_to_query_param(&cond).is_none());
    }

    #[test]
    fn array_value_returns_none() {
        let cond = eq_condition("tags", vec![CborValue::Text("a".into())]);
        assert!(condition_to_query_param(&cond).is_none());
    }
}
