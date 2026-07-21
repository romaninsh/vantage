//! YAML-facing schema for vista.
//!
//! A driver factory parses a `VistaSpec<T, C, R>` from YAML and lowers it
//! into a `Vista` via `VistaFactory::build_from_spec`. The three type
//! parameters carry driver-specific YAML blocks at the table, column, and
//! reference level. Each parameter defaults to [`NoExtras`] for drivers
//! that don't need any.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::reference::{ContainedKind, ReferenceKind};

/// Empty extras placeholder. Serializes as an absent key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
pub struct VistaSpec<T = NoExtras, C = NoExtras, R = NoExtras> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
    pub columns: IndexMap<String, ColumnSpec<C>>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub references: IndexMap<String, ReferenceSpec<R>>,
    /// Contained (embedded-in-row) relations, keyed by relation name. Each is
    /// lowered into a `with_contained_one`/`with_contained_many` registration.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub contained: IndexMap<String, ContainedYaml<C>>,
    #[serde(flatten, default)]
    pub driver: T,
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
pub struct ContainedYaml<C = NoExtras> {
    /// Column on the parent row holding the embedded object (one) or array (many).
    pub host_column: String,
    /// `contains_one` or `contains_many`.
    #[serde(default)]
    pub kind: ContainedKind,
    /// Field used as each contained record's id (`None` → positional index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
    /// The contained record's columns.
    pub columns: IndexMap<String, ColumnSpec<C>>,
}

/// Per-column metadata in a `VistaSpec`.
///
/// `flags` is open and unvalidated — the constants in [`crate::flags`]
/// name the values understood by vista's own accessors.
/// `references` holds the sugar form (`references: products`) when the
/// foreign key is the column itself; full reference declarations live in
/// the parent `VistaSpec::references` map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C: Serialize",
    deserialize = "C: DeserializeOwned + Default"
))]
pub struct ColumnSpec<C = NoExtras> {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub col_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<ReferenceSugar>,
    /// Rhai script for a *lazy* computed column. Runs in Rust on each
    /// returned record — the record as built so far is exposed as `row`,
    /// and the script's final expression becomes this column's value.
    /// Lazy columns apply in declaration order, so a later one sees the
    /// values earlier ones produced. Lowered via
    /// `Table::add_lazy_expression`; never part of the backend query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lazy: Option<String>,
    /// Rhai script for a *server-side* computed column. Evaluated once at
    /// build time with the driver's expression vocabulary (`ident(...)`,
    /// operators, `expr("raw")`, …) into a query expression, then lowered
    /// via `Table::with_expression` — the backend projects it as
    /// `(<expr>) AS <column>`. Unlike `lazy:` it participates in the
    /// query, so it can traverse record links or call backend functions.
    /// Computed columns are read-only.
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

    pub fn with_flag(mut self, flag: impl Into<String>) -> Self {
        self.flags.push(flag.into());
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
pub struct ReferenceSpec<R = NoExtras> {
    pub table: String,
    #[serde(default)]
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

/// One condition contributed by a reference during row-supplied
/// traversal: read the parent (locked) row's `from` column and constrain
/// the child's `to` column to that value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinKey {
    /// Child column to constrain.
    pub to: String,
    /// Parent column whose value (from the locked source row) is used.
    pub from: String,
}

/// Sugar form for inline column references.
///
/// `references: products` deserializes as `Sugar("products")`, equivalent
/// to a `has_one` reference with `foreign_key` defaulting to the column
/// name. Anything richer uses the full form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReferenceSugar {
    Sugar(String),
    Full(ReferenceSpec<NoExtras>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct DummyTable {
        dummy: DummyTableBlock,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct DummyTableBlock {
        path: String,
    }

    type DummySpec = VistaSpec<DummyTable, NoExtras, NoExtras>;

    #[test]
    fn parses_minimal_spec() {
        let yaml = r#"
name: clients
columns:
  id:
    type: int
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
dummy:
  path: data/clients.csv
"#;
        let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "clients");
        assert_eq!(spec.columns.len(), 2);
        assert_eq!(spec.columns["id"].col_type.as_deref(), Some("int"));
        assert_eq!(spec.columns["name"].flags, vec!["title", "searchable"]);
        assert_eq!(spec.driver.dummy.path, "data/clients.csv");
    }

    #[test]
    fn rejects_unknown_driver_field() {
        let yaml = r#"
name: clients
columns:
  id: { type: int }
dummy:
  path: x
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<DummySpec>(yaml).unwrap_err();
        assert!(
            err.to_string().contains("bogus") || err.to_string().contains("unknown"),
            "expected typo-detecting error, got: {err}"
        );
    }

    #[test]
    fn reference_sugar_round_trip() {
        let yaml = r#"
name: clients
columns:
  shop_id:
    type: int
    references: shops
dummy:
  path: x
"#;
        let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
        match spec.columns["shop_id"].references.as_ref().unwrap() {
            ReferenceSugar::Sugar(s) => assert_eq!(s, "shops"),
            other => panic!("expected sugar, got {other:?}"),
        }
    }

    #[test]
    fn contained_section_round_trip() {
        let yaml = r#"
name: order
columns:
  id: { type: string, flags: [id] }
  lines: { type: string }
contained:
  lines:
    host_column: lines
    kind: contains_many
    id_column: line_id
    columns:
      line_id: { type: string, flags: [id] }
      product: { type: string }
      quantity: { type: int }
dummy:
  path: x
"#;
        let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
        let c = &spec.contained["lines"];
        assert_eq!(c.host_column, "lines");
        assert_eq!(c.kind, ContainedKind::ContainsMany);
        assert_eq!(c.id_column.as_deref(), Some("line_id"));
        assert_eq!(c.columns.len(), 3);
        assert_eq!(c.columns["quantity"].col_type.as_deref(), Some("int"));
    }

    #[test]
    fn spec_without_contained_still_parses() {
        let yaml = r#"
name: clients
columns:
  id: { type: int, flags: [id] }
dummy:
  path: x
"#;
        let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.contained.is_empty());
    }

    #[test]
    fn full_reference_form() {
        let yaml = r#"
name: orders
columns:
  user_id:
    type: int
    references:
      table: users
      kind: has_one
      foreign_key: user_id
dummy:
  path: x
"#;
        let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
        match spec.columns["user_id"].references.as_ref().unwrap() {
            ReferenceSugar::Full(r) => {
                assert_eq!(r.table, "users");
                assert_eq!(r.kind, ReferenceKind::HasOne);
                assert_eq!(r.foreign_key.as_deref(), Some("user_id"));
            }
            other => panic!("expected full, got {other:?}"),
        }
    }
}
