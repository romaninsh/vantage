//! Conditions evaluated in memory, for a root field that takes no filter
//! argument.
//!
//! Vista treats equality push-down as universal, so a consumer narrowing
//! such a table never learns the server can't do it. Dropping the condition
//! at render time and returning every row would silently widen the result
//! — a relation tab would list every parent's children — so the driver
//! honours it here instead.

use serde_json::Value;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::expressive::ExpressiveEnum;

use crate::graphql::condition::{FieldCondition, GraphqlCondition, GraphqlOp};
use crate::graphql::impls::search::unescape_like;
use crate::graphql::types::GraphqlType as _;

/// Resolve a dotted path against a row. A missing or null link yields
/// `None`, so `run.commit.hash` on a run with no commit is absent rather
/// than an error.
pub(crate) fn pluck<'a>(row: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cursor = row;
    for segment in path.split('.') {
        cursor = cursor.get(segment)?;
    }
    Some(cursor)
}

/// Keep the rows every condition matches.
pub(crate) async fn retain_matching(
    rows: Vec<Value>,
    conditions: &[GraphqlCondition],
) -> Result<Vec<Value>> {
    // Resolved up front: a deferred value fetches the parent table, so
    // resolving per row would issue one round trip per row and could
    // compare different rows against different values.
    let conditions = resolve_deferred(conditions).await?;
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        if conditions.iter().all(|c| matches_condition(&row, c)) {
            kept.push(row);
        }
    }
    Ok(kept)
}

/// Replace every `DeferredField` with the plain `Field` it resolves to,
/// walking nested groups. `Deferred` produces a dialect-shaped filter
/// object rather than `field op value`, so there is nothing to evaluate
/// against a row — refusing beats quietly keeping every row.
fn resolve_deferred<'a>(
    conditions: &'a [GraphqlCondition],
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GraphqlCondition>>> + Send + 'a>>
{
    Box::pin(async move {
        let mut out = Vec::with_capacity(conditions.len());
        for condition in conditions {
            out.push(match condition {
                GraphqlCondition::DeferredField {
                    field,
                    op,
                    value_fn,
                } => {
                    let value = match value_fn.call().await? {
                        ExpressiveEnum::Scalar(v) => v.to_json(),
                        other => {
                            return Err(error!(
                                "Deferred filter resolved to a non-scalar",
                                got = format!("{:?}", other)
                            ));
                        }
                    };
                    GraphqlCondition::Field(FieldCondition::new(field.clone(), op.clone(), value))
                }
                GraphqlCondition::And(parts) => {
                    GraphqlCondition::And(resolve_deferred(parts).await?)
                }
                GraphqlCondition::Or(parts) => GraphqlCondition::Or(resolve_deferred(parts).await?),
                GraphqlCondition::Not(inner) => GraphqlCondition::Not(Box::new(
                    resolve_deferred(std::slice::from_ref(inner.as_ref()))
                        .await?
                        .remove(0),
                )),
                GraphqlCondition::Deferred(_) => {
                    return Err(error!(
                        "Deferred filter cannot be applied client-side; \
                         this root field accepts no filter argument"
                    ));
                }
                plain => plain.clone(),
            });
        }
        Ok(out)
    })
}

/// Evaluate a deferred-free condition against one row.
fn matches_condition(row: &Value, condition: &GraphqlCondition) -> bool {
    match condition {
        GraphqlCondition::Field(fc) => compare(row, &fc.field, &fc.op, &fc.value),
        GraphqlCondition::And(parts) => parts.iter().all(|p| matches_condition(row, p)),
        GraphqlCondition::Or(parts) => {
            parts.is_empty() || parts.iter().any(|p| matches_condition(row, p))
        }
        GraphqlCondition::Not(inner) => !matches_condition(row, inner),
        // `resolve_deferred` removed these before the row loop.
        GraphqlCondition::DeferredField { .. } | GraphqlCondition::Deferred(_) => false,
    }
}

/// Compare one row's value at `path` against `expected`. Ordering
/// comparisons work on numbers and strings; everything else falls back to
/// structural equality.
fn compare(row: &Value, path: &str, op: &GraphqlOp, expected: &Value) -> bool {
    let actual = pluck(row, path).unwrap_or(&Value::Null);
    let ordering = || match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => match (actual.as_str(), expected.as_str()) {
            (Some(a), Some(b)) => Some(a.cmp(b)),
            _ => None,
        },
    };
    let contains = || match expected {
        Value::Array(items) => items.iter().any(|item| item == actual),
        single => single == actual,
    };
    let text_match = |case_sensitive: bool| match (actual.as_str(), expected.as_str()) {
        (Some(a), Some(pattern)) => {
            if case_sensitive {
                like_matches(a, pattern)
            } else {
                like_matches(&a.to_lowercase(), &pattern.to_lowercase())
            }
        }
        _ => false,
    };
    match op {
        GraphqlOp::Eq => actual == expected,
        GraphqlOp::Ne => actual != expected,
        GraphqlOp::Gt => matches!(ordering(), Some(std::cmp::Ordering::Greater)),
        GraphqlOp::Gte => matches!(
            ordering(),
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
        ),
        GraphqlOp::Lt => matches!(ordering(), Some(std::cmp::Ordering::Less)),
        GraphqlOp::Lte => matches!(
            ordering(),
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
        ),
        GraphqlOp::In => contains(),
        GraphqlOp::NotIn => !contains(),
        GraphqlOp::Like => text_match(true),
        GraphqlOp::ILike => text_match(false),
        GraphqlOp::IsNull => actual.is_null(),
        GraphqlOp::IsNotNull => !actual.is_null(),
    }
}

/// LIKE anchors decide the shape of the test: `%foo%` is a substring,
/// `foo%` a prefix, `%foo` a suffix, and a bare `foo` an exact match.
/// Collapsing all four to `contains` would match rows the server excludes.
/// A backslash-escaped `%`, `_` or `\` is the character itself; an interior
/// unescaped `%` or `_` is compared literally — narrower than the server,
/// which is the safe direction to be wrong in.
fn like_matches(text: &str, pattern: &str) -> bool {
    let leading = pattern.starts_with('%');
    let body = if leading { &pattern[1..] } else { pattern };
    // A final `%` is an anchor unless an odd run of backslashes escapes it.
    let trailing = body.ends_with('%')
        && body[..body.len() - 1]
            .chars()
            .rev()
            .take_while(|c| *c == '\\')
            .count()
            % 2
            == 0;
    let body = if trailing {
        &body[..body.len() - 1]
    } else {
        body
    };
    let core = unescape_like(body);
    match (leading, trailing) {
        (true, true) => text.contains(&core),
        (true, false) => text.ends_with(&core),
        (false, true) => text.starts_with(&core),
        (false, false) => text == core,
    }
}
