//! YAML-facing schema for vista.
//!
//! A driver factory parses a `VistaSpec<T, C, R>` from YAML and lowers it
//! into a `Vista` via `VistaFactory::build_from_spec`. The three type
//! parameters carry driver-specific YAML blocks at the table, column, and
//! reference level. Each parameter defaults to [`NoExtras`] for drivers
//! that don't need any.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::reference::ReferenceKind;

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
    #[serde(flatten, default)]
    pub driver: T,
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
    #[serde(flatten, default)]
    pub driver: C,
}

impl<C: Default> ColumnSpec<C> {
    pub fn new() -> Self {
        Self {
            col_type: None,
            flags: Vec::new(),
            references: None,
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
