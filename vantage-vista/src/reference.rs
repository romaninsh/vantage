use serde::{Deserialize, Serialize};
use vantage_core::Result;

mod join_key;
mod narrow;
mod validate;

pub use join_key::JoinKey;

/// A relation a Vista exposes: a join to another table, or records contained
/// in a column of this table's rows. Traversed with
/// [`Vista::get_ref`](crate::Vista::get_ref).
///
/// A join (`HasOne`, `HasMany`) narrows the `target` table by its `keys`:
/// each [`JoinKey`] reads the parent row's `from` column and constrains the
/// target's `to` column to that value. A contained relation (`ContainsOne`,
/// `ContainsMany`) reads the parent row's `host_column` and has no keys.
///
/// Build one with the constructor of its kind, so the kind and its fields
/// agree. Deserialising checks the same agreement: a contained kind needs a
/// `host_column` and no keys, a join needs at least one key and no
/// `host_column`, and only a join takes a `build_script`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "validate::RawReference")]
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
    /// Rhai script that builds a join's traversal target in place of the
    /// keys, evaluated with the parent `row` in scope (`rhai` feature).
    /// Lowered from the per-reference YAML extras slot by each backend
    /// factory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_script: Option<String>,
}

impl Reference {
    /// One `target` row, found through `key` on the parent row. A join on
    /// several keys is lowered from YAML by
    /// [`ReferenceSpec::to_reference`](crate::ReferenceSpec::to_reference).
    pub fn has_one(name: impl Into<String>, target: impl Into<String>, key: JoinKey) -> Self {
        Self::join(name.into(), ReferenceKind::HasOne, target.into(), key)
    }

    /// The `target` rows whose `key` matches the parent row. A join on
    /// several keys is lowered from YAML by
    /// [`ReferenceSpec::to_reference`](crate::ReferenceSpec::to_reference).
    pub fn has_many(name: impl Into<String>, target: impl Into<String>, key: JoinKey) -> Self {
        Self::join(name.into(), ReferenceKind::HasMany, target.into(), key)
    }

    /// One more key a join narrows by; every key must match.
    pub(crate) fn with_key(mut self, key: JoinKey) -> Self {
        self.keys.push(key);
        self
    }

    /// One record embedded as an object in the parent's `host_column`.
    pub fn contains_one(name: impl Into<String>, host_column: impl Into<String>) -> Self {
        Self::contained(name.into(), ReferenceKind::ContainsOne, host_column.into())
    }

    /// Records embedded as an array in the parent's `host_column`.
    pub fn contains_many(name: impl Into<String>, host_column: impl Into<String>) -> Self {
        Self::contained(name.into(), ReferenceKind::ContainsMany, host_column.into())
    }

    /// Attach a Rhai build script; see [`build_script`](Self::build_script).
    /// A contained relation has no traversal target to build, so it is a
    /// plain error naming the relation.
    pub fn with_build_script(mut self, script: impl Into<String>) -> Result<Self> {
        validate::check_build_script(&self.name, self.kind)?;
        self.build_script = Some(script.into());
        Ok(self)
    }

    /// The foreign-key column of a single-key join: on the parent row for
    /// `HasOne` (the key's `from`), on the target for `HasMany` (its `to`).
    /// `None` for a contained relation or a join with several keys.
    pub(crate) fn foreign_key(&self) -> Option<&str> {
        match (self.kind, self.keys.as_slice()) {
            (ReferenceKind::HasOne, [key]) => Some(&key.from),
            (ReferenceKind::HasMany, [key]) => key.to.as_deref(),
            _ => None,
        }
    }

    fn join(name: String, kind: ReferenceKind, target: String, key: JoinKey) -> Self {
        Self {
            name,
            kind,
            target,
            keys: vec![key],
            host_column: None,
            build_script: None,
        }
    }

    fn contained(name: String, kind: ReferenceKind, host_column: String) -> Self {
        Self {
            target: name.clone(),
            name,
            kind,
            keys: Vec::new(),
            host_column: Some(host_column),
            build_script: None,
        }
    }
}

/// What a relation points at, and how many records it yields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReferenceKind {
    /// One target row, found by a key on the parent row.
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

    /// The kind as YAML spells it: `has_one`, `has_many`, `contains_one`,
    /// `contains_many`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::HasOne => "has_one",
            ReferenceKind::HasMany => "has_many",
            ReferenceKind::ContainsOne => "contains_one",
            ReferenceKind::ContainsMany => "contains_many",
        }
    }
}
