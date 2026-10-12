use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};

use crate::vista::Vista;

mod narrow;

/// A relation a Vista exposes: a join to another table, or records contained
/// in a column of this table's rows. Traversed with
/// [`Vista::get_ref`](crate::Vista::get_ref).
///
/// A join (`HasOne`, `HasMany`) narrows the `target` table by its `keys`:
/// each [`JoinKey`] reads the parent row's `from` column and constrains the
/// target's `to` column to that value. A contained relation (`ContainsOne`,
/// `ContainsMany`) reads the parent row's `host_column` and has no keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Reference {
    pub name: String,
    pub kind: ReferenceKind,
    /// The target table; for a contained relation, the relation's own name.
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<JoinKey>,
    /// Contained kinds: the parent column holding the embedded object (one)
    /// or array of objects (many).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_column: Option<String>,
    /// Rhai script that builds the traversal target in place of the keys,
    /// evaluated with the parent `row` in scope (`rhai` feature). Lowered
    /// from the per-reference YAML extras slot by each backend factory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_script: Option<String>,
}

impl Reference {
    /// A join to `target` through `keys`.
    pub fn new(
        name: impl Into<String>,
        target: impl Into<String>,
        kind: ReferenceKind,
        keys: Vec<JoinKey>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            target: target.into(),
            keys,
            host_column: None,
            build_script: None,
        }
    }

    /// A contained relation: the records embedded in `host_column`. `kind`
    /// is [`ContainsOne`](ReferenceKind::ContainsOne) or
    /// [`ContainsMany`](ReferenceKind::ContainsMany).
    pub fn contained(
        name: impl Into<String>,
        kind: ReferenceKind,
        host_column: impl Into<String>,
    ) -> Self {
        let name = name.into();
        Self {
            target: name.clone(),
            name,
            kind,
            keys: Vec::new(),
            host_column: Some(host_column.into()),
            build_script: None,
        }
    }

    /// Attach a Rhai build script; see [`build_script`](Self::build_script).
    pub fn with_build_script(mut self, script: impl Into<String>) -> Self {
        self.build_script = Some(script.into());
        self
    }

    /// The foreign-key column of a single-key join: on the parent row for
    /// `HasOne` (the key's `from`), on the target for `HasMany` (its `to`).
    /// `None` for a contained relation or a join with several keys.
    pub fn foreign_key(&self) -> Option<&str> {
        match (self.kind, self.keys.as_slice()) {
            (ReferenceKind::HasOne, [key]) => Some(&key.from),
            (ReferenceKind::HasMany, [key]) => key.to.as_deref(),
            _ => None,
        }
    }
}

/// What a relation points at, and how many records it yields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReferenceKind {
    /// One target row, found by a key on the parent row.
    #[default]
    HasOne,
    /// Target rows that carry the parent's key.
    HasMany,
    /// One record embedded as an object in a parent column.
    ContainsOne,
    /// Records embedded as an array in a parent column.
    ContainsMany,
}

impl ReferenceKind {
    /// Whether the relation's records live inside the parent row.
    pub fn is_contained(self) -> bool {
        matches!(
            self,
            ReferenceKind::ContainsOne | ReferenceKind::ContainsMany
        )
    }
}

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
