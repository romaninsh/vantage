//! YAML-facing schema for vista.
//!
//! A driver factory parses a `VistaSpec<T, C, R>` from YAML and lowers it
//! into a `Vista` via `VistaFactory::build_from_spec`. The three type
//! parameters carry driver-specific YAML blocks at the table, column, and
//! reference level. Each parameter defaults to [`NoExtras`] for drivers
//! that don't need any.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use vantage_core::Result;

use crate::column::{Column, ColumnFlag};
use crate::reference::{JoinKey, ReferenceKind};

mod lowering;
pub use lowering::{SpecResolver, resolve_base_spec};

/// Empty extras placeholder. Serializes as an absent key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct NoExtras {}

/// The YAML schema a vista is built from.
///
/// `T` carries the driver's table-level block (e.g. `csv: { path }`).
/// `C` carries each column's driver block. `R` carries each reference's
/// driver block. The outer struct cannot use `deny_unknown_fields` because
/// of `#[serde(flatten)]`; the driver-specific extras struct should set
/// `deny_unknown_fields` itself to catch typos in driver blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T: Serialize, C: Serialize, R: Serialize",
    deserialize = "T: DeserializeOwned + Default, C: DeserializeOwned + Default, R: DeserializeOwned + Default"
))]
#[non_exhaustive]
pub struct VistaSpec<T = NoExtras, C = NoExtras, R = NoExtras> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
    pub columns: IndexMap<String, ColumnSpec<C>>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub references: IndexMap<String, ReferenceSpec<R>>,
    /// Contained (embedded-in-row) relations, keyed by relation name. Each
    /// becomes a reference of a contained kind.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub contained: IndexMap<String, ContainedYaml<C>>,
    #[serde(flatten, default)]
    pub driver: T,
}

impl<T: Default, C, R> VistaSpec<T, C, R> {
    /// A spec named `name` that declares nothing else: no columns,
    /// references or contained relations, and the driver's default block.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            datasource: None,
            id_column: None,
            columns: IndexMap::new(),
            references: IndexMap::new(),
            contained: IndexMap::new(),
            driver: T::default(),
        }
    }
}

/// YAML shape of a contained relation — embedded records stored in a column of
/// the parent row. Mirrors [`ReferenceSpec`] but carries the contained
/// record's own column schema (built via the driver's `build_column`) instead
/// of a foreign key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "C: Serialize",
        deserialize = "C: DeserializeOwned + Default"
    )
)]
#[non_exhaustive]
pub struct ContainedYaml<C = NoExtras> {
    /// Column on the parent row holding the embedded object (one) or array (many).
    pub host_column: String,
    /// `contains_one` (the default) or `contains_many`.
    #[serde(default = "contains_one")]
    pub kind: ReferenceKind,
    /// Field used as each contained record's id (`None` → positional index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
    /// The contained record's columns.
    pub columns: IndexMap<String, ColumnSpec<C>>,
}

fn contains_one() -> ReferenceKind {
    ReferenceKind::ContainsOne
}

fn has_one() -> ReferenceKind {
    ReferenceKind::HasOne
}

/// Per-column metadata in a `VistaSpec`.
///
/// `flags` are snake_case names; a name vista doesn't know is kept as
/// [`ColumnFlag::Other`] for the layer that owns the YAML schema to accept
/// or reject.
/// `references` holds the sugar form (`references: products`) when the
/// foreign key is the column itself; full reference declarations live in
/// the parent `VistaSpec::references` map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C: Serialize",
    deserialize = "C: DeserializeOwned + Default"
))]
#[non_exhaustive]
pub struct ColumnSpec<C = NoExtras> {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub col_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ColumnFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<ReferenceSugar>,
    /// Rhai script for a *lazy* computed column. Runs in Rust on each
    /// returned record — the record as built so far is exposed as `row`,
    /// and the script's final expression becomes this column's value.
    /// Lazy columns apply in declaration order, so a later one sees the
    /// values earlier ones produced. Lowered via
    /// [`lazy_column`](Self::lazy_column) onto a Vista computed column;
    /// never part of the backend query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lazy: Option<String>,
    /// Rhai script for a *server-side* computed column. Evaluated once at
    /// build time with the driver's expression vocabulary
    /// (`surreal::ident(...)`, operators, `surreal::expr("raw")`, …) into a
    /// query expression, then lowered via `Table::with_expression` — the
    /// backend projects it as `(<expr>) AS <column>`. Unlike `lazy:` it
    /// participates in the query, so it can traverse record links or call
    /// backend functions. Computed columns are read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    #[serde(flatten, default)]
    pub driver: C,
}

impl<C: Default> ColumnSpec<C> {
    pub fn new() -> Self {
        Self {
            col_type: None,
            flags: Vec::new(),
            references: None,
            lazy: None,
            expr: None,
            driver: C::default(),
        }
    }

    pub fn with_type(mut self, ty: impl Into<String>) -> Self {
        self.col_type = Some(ty.into());
        self
    }

    pub fn with_flag(mut self, flag: impl Into<ColumnFlag>) -> Self {
        self.flags.push(ColumnFlag::from(flag.into().as_str()));
        self
    }

    pub fn with_lazy(mut self, script: impl Into<String>) -> Self {
        self.lazy = Some(script.into());
        self
    }

    pub fn with_expr(mut self, script: impl Into<String>) -> Self {
        self.expr = Some(script.into());
        self
    }
}

impl<C> ColumnSpec<C> {
    /// The Vista column for a `lazy:` column — its type and flags, computed
    /// by its script (see `Column::with_expression`) — or `None` for a
    /// stored column. Errors when the script doesn't compile, or when this
    /// crate was built without the `rhai` feature.
    pub fn lazy_column(&self, name: &str) -> Result<Option<Column>> {
        let Some(code) = &self.lazy else {
            return Ok(None);
        };
        let typed = match &self.col_type {
            Some(ty) => Column::new(name, ty),
            None => Column::untyped(name),
        };
        let column = self.flags.iter().cloned().fold(typed, Column::with_flag);
        computed(column, code).map(Some)
    }
}

#[cfg(feature = "rhai")]
fn computed(column: Column, code: &str) -> Result<Column> {
    column.with_expression(code)
}

#[cfg(not(feature = "rhai"))]
fn computed(column: Column, _code: &str) -> Result<Column> {
    Err(vantage_core::error!(
        "Column declares a `lazy:` script but vantage-vista was built without the `rhai` feature",
        column = column.name
    ))
}

impl<C: Default> Default for ColumnSpec<C> {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level reference declaration in a `VistaSpec`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "R: Serialize",
    deserialize = "R: DeserializeOwned + Default"
))]
#[non_exhaustive]
pub struct ReferenceSpec<R = NoExtras> {
    pub table: String,
    /// `has_one` or `has_many`; defaults to `has_one`.
    #[serde(default = "has_one")]
    pub kind: ReferenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_key: Option<String>,
    /// Extra join keys for row-supplied traversal. When non-empty these
    /// fully describe the join: each maps a parent (locked-row) column to
    /// a child column, and the child is constrained by all of them. This
    /// lets a child be narrowed by more than one parent field (e.g. a
    /// deployment narrowed by both `product_id` and `version_id`). When
    /// empty, the single `foreign_key` sugar applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<JoinKey>,
    #[serde(flatten, default)]
    pub driver: R,
}

/// Sugar form for inline column references.
///
/// `references: products` deserializes as `Sugar("products")`, equivalent
/// to a `has_one` reference with `foreign_key` defaulting to the column
/// name. Anything richer uses the full form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ReferenceSugar {
    Sugar(String),
    Full(ReferenceSpec<NoExtras>),
}
