//! Conditions for Kubernetes-backed Vantage tables.
//!
//! The Kubernetes list API only filters server-side by label/field
//! selectors, so for v1 every condition is applied **client-side** after
//! the projector has flattened each object. `Eq` is the only operator we
//! evaluate; `In` and `Deferred` exist purely to make `with_one` /
//! `with_many` traversal work — they must collapse to a single value at
//! execute time, otherwise the call errors loudly (same contract as
//! `vantage-aws`).

use ciborium::Value as CborValue;
use vantage_expressions::Expression;

#[derive(Clone)]
pub enum KubeCondition {
    /// `field == value`. Evaluated against the projected (flat) record.
    Eq { field: String, value: CborValue },
    /// `field == value` from a literal set. A single-element set
    /// collapses to `Eq`; zero or multi-element is a hard error.
    In {
        field: String,
        values: Vec<CborValue>,
    },
    /// `field == value` where the value comes from another query.
    /// Resolved at execute time; the source must yield exactly one value.
    Deferred {
        field: String,
        source: Expression<CborValue>,
    },
}

// Manual Debug — `Expression<CborValue>` doesn't impl Debug because
// `ciborium::Value` doesn't impl Display.
impl std::fmt::Debug for KubeCondition {
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

impl KubeCondition {
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

/// `field == value`. Shorthand for [`KubeCondition::eq`].
pub fn eq(field: impl Into<String>, value: impl Into<CborValue>) -> KubeCondition {
    KubeCondition::eq(field, value)
}

/// `field IN values` (literal set). Shorthand for [`KubeCondition::in_`].
/// A multi-element set errors at execute time.
pub fn in_<I, V>(field: impl Into<String>, values: I) -> KubeCondition
where
    I: IntoIterator<Item = V>,
    V: Into<CborValue>,
{
    KubeCondition::in_(field, values)
}

/// Collapse conditions to `(field, value)` equality pairs for client-side
/// matching. Errors on zero- or multi-element `In`; a `Deferred` reaching
/// here is an internal error (it must be resolved upstream).
pub(crate) fn eq_pairs(conditions: &[KubeCondition]) -> vantage_core::Result<Vec<(String, CborValue)>> {
    let mut out = Vec::with_capacity(conditions.len());
    for cond in conditions {
        match cond {
            KubeCondition::Eq { field, value } => out.push((field.clone(), value.clone())),
            KubeCondition::In { field, values } => match values.as_slice() {
                [single] => out.push((field.clone(), single.clone())),
                [] => {
                    return Err(vantage_core::error!(
                        "KubeCondition::In with zero values is not representable",
                        field = field.as_str()
                    ));
                }
                _ => {
                    return Err(vantage_core::error!(
                        "KubeCondition::In with more than one value is not supported — \
                         relations must traverse from a single parent",
                        field = field.as_str(),
                        count = values.len()
                    ));
                }
            },
            KubeCondition::Deferred { field, .. } => {
                return Err(vantage_core::error!(
                    "Internal: Deferred condition reached matcher unresolved",
                    field = field.as_str()
                ));
            }
        }
    }
    Ok(out)
}
