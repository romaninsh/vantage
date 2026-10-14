//! Condition type for redb tables.
//!
//! redb has no query language, so conditions are minimal: `Eq`, `In`, and
//! `Deferred`, which resolves a `DeferredFn` at execution time into an `In`
//! (relationship traversal builds these).
//!
//! The first condition of a read must name an `Indexed` column or the id
//! column; otherwise the read panics.

use vantage_expressions::{DeferredFn, ExpressiveEnum};

use crate::types::AnyRedbType;

/// A redb filter clause.
#[derive(Clone)]
pub enum RedbCondition {
    /// `column == value` — resolved via index lookup on `column`.
    Eq { column: String, value: AnyRedbType },
    /// `column IN (values)` — multiple index lookups merged in memory.
    In {
        column: String,
        values: Vec<AnyRedbType>,
    },
    /// Resolved async at execution time into an `In`; see [`RedbCondition::resolve`].
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

    /// Resolve a deferred condition into an `In`; a payload that isn't a
    /// `[column, values]` tuple is an error. Other conditions return unchanged.
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
                // Wire shape: CBOR `[Text(column), Array(values)]`.
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
