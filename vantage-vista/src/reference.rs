use serde::{Deserialize, Serialize};

use crate::column::Column;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub target: String,
    pub kind: ReferenceKind,
    pub foreign_key: String,
    /// Optional Rhai script that *builds* the traversal target itself, in place
    /// of the default foreign-key eq-condition path. Evaluated lazily at
    /// traversal time with the parent `row` in scope (see
    /// [`crate::rhai_conventional`]). `None` keeps the conventional FK path.
    /// Lowered from the per-reference YAML extras slot by each backend factory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_script: Option<String>,
}

/// Schema for a **contained** relation — records embedded in a column of the
/// parent row rather than stored in a separate table.
///
/// Unlike [`Reference`] (which names a foreign-key column), a contained
/// relation names the **host column** holding the embedded data, carries the
/// contained record's own column schema, and an optional id column. With no id
/// column, contained-many records are addressed by positional index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainedSpec {
    pub name: String,
    /// Column on the parent row holding the embedded object (one) or array
    /// of objects (many).
    pub host_column: String,
    /// [`ContainsOne`](ContainedKind::ContainsOne) or
    /// [`ContainsMany`](ContainedKind::ContainsMany).
    pub kind: ContainedKind,
    /// Columns of the contained record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    /// Field used as the contained record's id. `None` → positional index
    /// (contained-many) or the fixed relation name (contained-one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
}

impl ContainedSpec {
    pub fn new(
        name: impl Into<String>,
        host_column: impl Into<String>,
        kind: ContainedKind,
    ) -> Self {
        Self {
            name: name.into(),
            host_column: host_column.into(),
            kind,
            columns: Vec::new(),
            id_column: None,
        }
    }

    pub fn with_columns(mut self, columns: Vec<Column>) -> Self {
        self.columns = columns;
        self
    }

    pub fn with_id_column(mut self, id_column: impl Into<String>) -> Self {
        self.id_column = Some(id_column.into());
        self
    }
}

impl Reference {
    pub fn new(
        name: impl Into<String>,
        target: impl Into<String>,
        kind: ReferenceKind,
        foreign_key: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            kind,
            foreign_key: foreign_key.into(),
            build_script: None,
        }
    }

    /// Attach a Rhai build script that constructs the traversal target,
    /// overriding the default foreign-key eq-condition path. See
    /// [`build_script`](Self::build_script).
    pub fn with_build_script(mut self, script: impl Into<String>) -> Self {
        self.build_script = Some(script.into());
        self
    }
}

/// Cardinality of a relation. Cross-persistence-ness is no longer
/// encoded here — it's determined at resolution time by whether the
/// target Vista lives in the same driver or a different one (the
/// inventory loader knows). YAML specs that previously used
/// `kind: has_foreign` migrate to `kind: has_one` or `kind: has_many`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    #[default]
    HasOne,
    HasMany,
}

/// Cardinality of a **contained** relation — records embedded in a column of
/// the parent row. Kept separate from [`ReferenceKind`] (which names
/// foreign-key references) so contained relations don't leak into the
/// foreign-key code paths: a contained relation is not a join, it's a view
/// onto one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainedKind {
    /// One record embedded as an object in the host column (e.g. a product's
    /// `inventory`).
    #[default]
    ContainsOne,
    /// Many records embedded as an array in the host column (e.g. an order's
    /// `lines`).
    ContainsMany,
}
