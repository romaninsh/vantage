//! Condition type for redb tables.
//!
//! redb is a key-value store with no query language, so the public condition
//! type is intentionally minimal — only `Eq` is exposed to callers; `In` is
//! used internally for relationship traversal and `Deferred` resolves a
//! `DeferredFn` at execution time (yielding either an `Eq` or `In` document).
//!
//! Conditions can only be added to columns flagged `Indexed`; the table source
//! panics at execution time if it sees a condition referencing an unflagged,
//! non-id column.

use vantage_expressions::{DeferredFn, ExpressiveEnum};

use crate::types::AnyRedbType;

/// A redb filter clause.
#[derive(Clone)]
pub enum RedbCondition {
    /// `column == value` — resolved via index lookup on `column`.
    Eq {
        column: String,
        value: AnyRedbType,
    },
    /// `column IN (values)` — multiple index lookups merged in memory.
    In {
        column: String,
        values: Vec<AnyRedbType>,
    },
    /// Deferred — resolved async at execution time. Must produce an `Eq` or
    /// `In` after resolution.
    Deferred(DeferredFn<AnyRedbType>),
}

impl RedbCondition {
    pub fn eq(column: impl Into<String>, value: impl Into<AnyRedbType>) -> Self {
        Self::Eq {
            column: column.into(),
            value: value.into(),
        }
    }

    pub fn in_<I, V>(column: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<AnyRedbType>,
    {
        Self::In {
            column: column.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// Resolve a deferred condition into an immediate one. Non-deferred
    /// conditions return self unchanged.
    pub async fn resolve(self) -> vantage_core::Result<Self> {
        match self {
            Self::Deferred(d) => {
                let resolved = d.call().await?;
                let any = match resolved {
                    ExpressiveEnum::Scalar(v) => v,
                    other => {
                        return Err(vantage_core::error!(
                            "Deferred RedbCondition produced non-scalar",
                            kind = format!("{:?}", std::mem::discriminant(&other))
                        ));
                    }
                };
                // The deferred side encodes a (column, [values]) pair as a
                // CBOR array `[Text(column), Array(values)]`.
                match any.into_value() {
                    ciborium::Value::Array(parts) if parts.len() == 2 => {
                        let mut iter = parts.into_iter();
                        let col = match iter.next() {
                            Some(ciborium::Value::Text(s)) => s,
                            _ => {
                                return Err(vantage_core::error!(
                                    "Deferred condition: expected column name as text"
                                ));
                            }
                        };
                        let values = match iter.next() {
                            Some(ciborium::Value::Array(items)) => items,
                            _ => {
                                return Err(vantage_core::error!(
                                    "Deferred condition: expected values as array"
                                ));
                            }
                        };
                        Ok(Self::In {
                            column: col,
                            values: values.into_iter().map(AnyRedbType::untyped).collect(),
                        })
                    }
                    _ => Err(vantage_core::error!(
                        "Deferred condition: expected [column, values] tuple"
                    )),
                }
            }
            other => Ok(other),
        }
    }

    /// Field name targeted by this condition (post-resolution).
    pub fn column(&self) -> Option<&str> {
        match self {
            Self::Eq { column, .. } | Self::In { column, .. } => Some(column.as_str()),
            Self::Deferred(_) => None,
        }
    }
}

impl std::fmt::Debug for RedbCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq { column, value } => write!(f, "Eq({} = {})", column, value),
            Self::In { column, values } => write!(f, "In({} ∈ {} values)", column, values.len()),
            Self::Deferred(_) => write!(f, "Deferred(...)"),
        }
    }
}
