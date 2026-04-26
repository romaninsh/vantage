//! Condition helpers — build eq-conditions for `Table<RestApi, _>` and
//! peel them apart at the `list_values` boundary into URL query params.
//!
//! Why not the standard `Operation::eq` from `vantage-table`? That trait
//! is blanket-implemented for all `Expressive<T>`, but we can't provide
//! the *value-side* `Expressive<serde_json::Value>` impls for primitive
//! types — orphan rule (both `Expressive` and `serde_json::Value` are
//! foreign to this crate). A future refactor wrapping the value type in
//! a local newtype would unlock the full `Operation` surface; for now
//! we provide an `eq_condition(field, value)` free function that builds
//! the same expression shape directly.

use serde_json::Value;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;

/// Build an `eq` condition `field = value` for use with `Table<RestApi,
/// _>::add_condition`. Produces the same shape as
/// `vantage_table::operation::Operation::eq` would for backends that
/// support it.
///
/// ```ignore
/// use vantage_api_client::eq_condition;
/// use serde_json::json;
///
/// let mut comments = Table::<RestApi, EmptyEntity>::new("comments", api);
/// comments.add_condition(eq_condition("postId", json!(1)));
/// ```
pub fn eq_condition(field: &str, value: Value) -> Expression<Value> {
    Expression::new(
        "{} = {}",
        vec![
            // Field side: bare identifier expression.
            ExpressiveEnum::Nested(Expression::new(field.to_string(), vec![])),
            // Value side: scalar wrapped in a `{}` expression so it
            // matches the layout `Operation::eq` produces.
            ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(value)])),
        ],
    )
}

/// Try to peel an `eq`-shaped condition into `(field_name, value_string)`
/// suitable for a URL query parameter.
///
/// Recognised shape — same one `eq_condition` (and
/// `vantage_table::Operation::eq` for compatible value types) produces:
///
/// ```text
/// Expression {
///     template: "{} = {}",
///     parameters: [
///         Nested(Expression { template: <field_name>, parameters: [] }),
///         Nested(Expression { template: "{}", parameters: [Scalar(value)] }),
///     ],
/// }
/// ```
///
/// Returns `None` for anything that doesn't match — non-eq operators,
/// nested column-on-column comparisons, deferred values, etc. The caller
/// decides what to do (skip silently is the v1 stance).
pub(crate) fn condition_to_query_param(cond: &Expression<Value>) -> Option<(String, String)> {
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
                ExpressiveEnum::Scalar(v) => json_to_query_string(v)?,
                _ => return None,
            }
        }
        ExpressiveEnum::Scalar(v) => json_to_query_string(v)?,
        _ => return None,
    };

    Some((field, value))
}

fn json_to_query_string(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eq_condition_round_trips_int() {
        let cond = eq_condition("userId", json!(1));
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("userId".into(), "1".into()));
    }

    #[test]
    fn eq_condition_round_trips_string() {
        let cond = eq_condition("name", json!("Alice"));
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("name".into(), "Alice".into()));
    }

    #[test]
    fn eq_condition_round_trips_bool() {
        let cond = eq_condition("completed", json!(true));
        let pair = condition_to_query_param(&cond).expect("eq parses");
        assert_eq!(pair, ("completed".into(), "true".into()));
    }

    #[test]
    fn raw_expression_with_unknown_template_returns_none() {
        let cond = Expression::<Value>::new("CUSTOM SQL", vec![]);
        assert!(condition_to_query_param(&cond).is_none());
    }

    #[test]
    fn array_value_returns_none() {
        let cond = eq_condition("tags", json!(["a", "b"]));
        // Arrays don't map to a single query-param string.
        assert!(condition_to_query_param(&cond).is_none());
    }
}
