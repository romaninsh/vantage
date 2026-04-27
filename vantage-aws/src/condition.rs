//! Conditions for AWS-backed Vantage tables.
//!
//! AWS APIs only accept exact-match filters, so the only operator
//! that survives the round-trip is equality. `In` and `Deferred` are
//! here to support `with_one` / `with_many` traversal — they must
//! collapse to a single value at execute time, otherwise the call
//! errors loudly.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::Expression;

#[derive(Clone)]
pub enum AwsCondition {
    /// `field == value`. Folds into the JSON request body verbatim.
    Eq { field: String, value: CborValue },
    /// `field == value` from a literal set. A single-element set
    /// collapses to `Eq`; zero or multi-element is a hard error.
    In {
        field: String,
        values: Vec<CborValue>,
    },
    /// `field == value` where the value comes from another query.
    /// Resolved at execute time; the source must yield exactly one
    /// value.
    Deferred {
        field: String,
        source: Expression<CborValue>,
    },
}

// Manual Debug — `Expression<CborValue>` doesn't impl Debug because
// `ciborium::Value` doesn't impl Display. We render structurally
// without leaning on the inner expression's own Debug.
impl std::fmt::Debug for AwsCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq { field, value } => f
                .debug_struct("Eq")
                .field("field", field)
                .field("value", value)
                .finish(),
            Self::In { field, values } => f
                .debug_struct("In")
                .field("field", field)
                .field("values", values)
                .finish(),
            Self::Deferred { field, source } => f
                .debug_struct("Deferred")
                .field("field", field)
                .field("source.template", &source.template)
                .field("source.params", &source.parameters.len())
                .finish(),
        }
    }
}

impl AwsCondition {
    pub fn eq(field: impl Into<String>, value: impl Into<CborValue>) -> Self {
        Self::Eq {
            field: field.into(),
            value: value.into(),
        }
    }

    pub fn in_<I, V>(field: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<CborValue>,
    {
        Self::In {
            field: field.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    pub fn field(&self) -> &str {
        match self {
            Self::Eq { field, .. } | Self::In { field, .. } | Self::Deferred { field, .. } => field,
        }
    }
}

/// `field == value`. Shorthand for [`AwsCondition::eq`].
///
/// ```
/// # use vantage_aws::eq;
/// let cond = eq("logGroupNamePrefix", "/aws/lambda/");
/// ```
pub fn eq(field: impl Into<String>, value: impl Into<CborValue>) -> AwsCondition {
    AwsCondition::eq(field, value)
}

/// `field IN values` (literal set). Shorthand for [`AwsCondition::in_`].
/// Remember the single-value rule — a multi-element set will error
/// at execute time.
pub fn in_<I, V>(field: impl Into<String>, values: I) -> AwsCondition
where
    I: IntoIterator<Item = V>,
    V: Into<CborValue>,
{
    AwsCondition::in_(field, values)
}

/// Fold a slice of conditions into a JSON object suitable for an AWS
/// JSON-1.1 request body. Errors if any `In` has zero or multi-element
/// values, and panics if a `Deferred` reached this point — `Deferred`
/// must be resolved into `Eq` *before* `build_body` runs (see
/// `AwsJson1::resolve_conditions`).
pub(crate) fn build_body(
    conditions: &[AwsCondition],
) -> vantage_core::Result<serde_json::Map<String, JsonValue>> {
    let mut body = serde_json::Map::new();
    for cond in conditions {
        match cond {
            AwsCondition::Eq { field, value } => {
                body.insert(field.clone(), cbor_to_json(value));
            }
            AwsCondition::In { field, values } => match values.as_slice() {
                [single] => {
                    body.insert(field.clone(), cbor_to_json(single));
                }
                [] => {
                    return Err(vantage_core::error!(
                        "AwsCondition::In with zero values is not representable",
                        field = field.as_str()
                    ));
                }
                _ => {
                    return Err(vantage_core::error!(
                        "AwsCondition::In with more than one value is not supported \
                         by AWS — relations must traverse from a single parent",
                        field = field.as_str(),
                        count = values.len()
                    ));
                }
            },
            AwsCondition::Deferred { field, .. } => {
                return Err(vantage_core::error!(
                    "Internal: Deferred condition reached build_body unresolved \
                     — AwsJson1::resolve_conditions should have materialised it",
                    field = field.as_str()
                ));
            }
        }
    }
    Ok(body)
}

/// CBOR → JSON via ciborium's serde bridge. Used at the wire boundary
/// when emitting request bodies. Falls back to `null` for the rare
/// CBOR shapes JSON can't represent (which AWS conditions don't
/// produce).
fn cbor_to_json(v: &CborValue) -> JsonValue {
    v.deserialized::<JsonValue>().unwrap_or(JsonValue::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eq_folds_into_body() {
        let conds = [eq("logGroupNamePrefix", "/aws/lambda/")];
        let body = build_body(&conds).unwrap();
        assert_eq!(body["logGroupNamePrefix"], json!("/aws/lambda/"));
    }

    #[test]
    fn single_element_in_collapses_to_eq() {
        let conds = [in_(
            "logGroupName",
            vec![CborValue::from("/aws/lambda/foo")],
        )];
        let body = build_body(&conds).unwrap();
        assert_eq!(body["logGroupName"], json!("/aws/lambda/foo"));
    }

    #[test]
    fn multi_element_in_errors() {
        let conds = [in_(
            "logGroupName",
            vec![CborValue::from("a"), CborValue::from("b")],
        )];
        let err = build_body(&conds).unwrap_err();
        assert!(format!("{err}").contains("more than one value"));
    }

    #[test]
    fn empty_in_errors() {
        let conds = [AwsCondition::In {
            field: "x".into(),
            values: vec![],
        }];
        assert!(build_body(&conds).is_err());
    }

    #[test]
    fn deferred_in_build_body_is_internal_error() {
        // build_body should never see Deferred — resolve_conditions
        // turns them into Eq first. If one slips through, surface it
        // loudly rather than silently dropping the filter.
        let conds = [AwsCondition::Deferred {
            field: "x".into(),
            source: Expression::new("noop", vec![]),
        }];
        let err = build_body(&conds).unwrap_err();
        assert!(format!("{err}").contains("Deferred"));
    }

    #[test]
    fn multiple_eqs_compose() {
        let conds = [
            eq("logGroupName", "/aws/lambda/foo"),
            eq("startTime", 1_700_000_000_000i64),
        ];
        let body = build_body(&conds).unwrap();
        assert_eq!(body["logGroupName"], json!("/aws/lambda/foo"));
        assert_eq!(body["startTime"], json!(1_700_000_000_000i64));
    }
}
