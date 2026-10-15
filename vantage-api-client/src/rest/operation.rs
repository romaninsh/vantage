//! Condition helpers — build eq-conditions for `Table<RestApi, _>` and
//! peel them apart at the `list_values` boundary into URL query params.
//!
//! `eq_condition` stands in for `vantage-table`'s `Operation::eq`: the
//! `Expressive<ciborium::Value>` impls for primitive types would have to live
//! in a foreign crate, so the helper builds the same expression shape
//! directly.

use ciborium::Value as CborValue;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;

/// Build an `eq` condition `field = value` for `Table<RestApi, _>`.
///
/// `value` is anything that converts into `CborValue`, which includes the
/// scalars: `eq_condition("userId", 1i64)`.
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

/// The `{placeholder}` names in an endpoint template, in order:
/// `teams/{team_id}/users` yields `team_id`. Scanning stops at a `{`
/// with no closing `}`.
pub(crate) fn placeholders(template: &str) -> impl Iterator<Item = &str> {
    let mut rest = template;
    std::iter::from_fn(move || {
        let open = rest.find('{')?;
        let after = &rest[open + 1..];
        let close = after.find('}')?;
        rest = &after[close + 1..];
        Some(&after[..close])
    })
}
