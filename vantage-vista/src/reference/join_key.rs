//! [`JoinKey`]: one column pair of a join.

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};

use crate::vista::Vista;

/// One condition a join contributes: read the parent row's `from` column and
/// constrain the target's `to` column to that value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct JoinKey {
    /// Parent column whose value is used.
    pub from: String,
    /// Target column to constrain; `None` is the target's id column, for a
    /// join whose target id is only known once the target is built (a
    /// has-one declared on a typed table). Resolve it with
    /// [`target_column`](Self::target_column).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl JoinKey {
    /// `JoinKey { from, to }`: the parent's `from` column constrains the
    /// target's `to` column.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: Some(to.into()),
        }
    }

    /// The parent's `from` column constrains the target's id column.
    pub fn to_target_id(from: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: None,
        }
    }

    /// The column of `target` this key constrains: `to`, or the target's id
    /// column when `to` is `None`. A target with no id column is an error.
    pub fn target_column<'a>(&'a self, target: &'a Vista) -> Result<&'a str> {
        match &self.to {
            Some(to) => Ok(to),
            None => target.id_column().ok_or_else(|| {
                error!(
                    "join target has no id column to constrain",
                    target = target.name(),
                    from = self.from.as_str()
                )
            }),
        }
    }
}
