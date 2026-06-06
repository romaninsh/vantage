//! Conditions for command-backed tables.
//!
//! Conditions are peeled into `{ field, op, value }` shapes and handed to
//! the Rhai script, which decides how (or whether) to translate each into
//! a CLI flag. As a safety net, plain `Eq` conditions are also re-applied
//! client-side against the returned rows (see `table_source`), mirroring
//! `vantage-aws`.
//!
//! `Deferred` exists only to make `with_one` / `with_many` traversal work:
//! it is resolved to `Eq` / `In` (by awaiting the embedded subquery) in
//! `list_table_values`, *before* the script ever sees the conditions.

use ciborium::Value as CborValue;
use vantage_expressions::Expression;

/// A filter on a command-backed table.
#[derive(Clone)]
pub enum CmdCondition {
    /// `field == value`.
    Eq { field: String, value: CborValue },
    /// `field in values` — a parent → child traversal usually yields one.
    In {
        field: String,
        values: Vec<CborValue>,
    },
    /// `field == value` where the value comes from another query. Resolved
    /// at read time before the script runs.
    Deferred {
        field: String,
        source: Expression<CborValue>,
    },
}

// Manual Debug — `Expression<CborValue>` doesn't impl Debug (ciborium's
// Value has no Display), so we render structurally.
impl std::fmt::Debug for CmdCondition {
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
                .finish(),
        }
    }
}

impl CmdCondition {
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

    /// The operator name exposed to Rhai (`"eq"` / `"in"`). Never called on
    /// `Deferred` — those are resolved before the script sees them.
    pub(crate) fn op(&self) -> &'static str {
        match self {
            Self::Eq { .. } => "eq",
            Self::In { .. } => "in",
            Self::Deferred { .. } => "eq",
        }
    }

    /// The value exposed to Rhai, as JSON: a scalar for `Eq`, an array for `In`.
    pub(crate) fn json_value(&self) -> serde_json::Value {
        match self {
            Self::Eq { value, .. } => crate::types::cbor_to_json(value),
            Self::In { values, .. } => {
                serde_json::Value::Array(values.iter().map(crate::types::cbor_to_json).collect())
            }
            Self::Deferred { .. } => serde_json::Value::Null,
        }
    }
}

/// `field == value`. Shorthand for [`CmdCondition::eq`].
///
/// ```
/// # use vantage_cmd::eq;
/// let cond = eq("logGroupNamePrefix", "/aws/lambda/");
/// ```
pub fn eq(field: impl Into<String>, value: impl Into<CborValue>) -> CmdCondition {
    CmdCondition::eq(field, value)
}
