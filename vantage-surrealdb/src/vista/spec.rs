//! YAML-facing types for the SurrealDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::VistaSpec;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealTableBlock {
    /// Override for the SurrealDB table name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// A Rhai script that builds the SELECT used as this vista's source,
    /// instead of a physical table. Produces a read-only vista. Requires the
    /// `rhai` feature. Mutually exclusive with `table`.
    ///
    /// When `base` is also set, the script runs in *transform mode*: the base
    /// vista's `select()` is seeded into the engine scope as `base`, so the
    /// script extends an existing query instead of building one from scratch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
    /// Name of another vista to derive this one from. Resolved eagerly at build
    /// time through the factory's spec resolver. The base's columns/relations
    /// are inherited per `inherit`, and the base `select()` is the starting
    /// point for the `rhai` transform. Produces a read-only vista.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Which of the base vista's columns/relations to inherit. Only meaningful
    /// alongside `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit: Option<InheritBlock>,
    /// A Rhai script applied to the *built* vista as a final step, after the
    /// YAML table (columns/references/source) is constructed. The vista is
    /// exposed as `self`; the script narrows or tweaks it with the conventional
    /// verbs plus vendor expression conditions YAML can't express, e.g.
    /// `self.with_condition(ident("is_paying_client") == true)`. Composes with
    /// `table`/`rhai`/`base` (it runs last). Requires the `rhai` feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<String>,
}

/// Selects which parts of a `base` vista a derived vista inherits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InheritBlock {
    /// Base column names to copy onto the derived table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// Base relation names to copy onto the derived table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealColumnBlock {
    /// SurrealDB field name when it differs from the spec column name.
    /// The vista surfaces values under the spec name; the underlying field
    /// is read/written under this alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// Per-reference SurrealDB block. Mirrors [`SurrealTableExtras`]/
/// [`SurrealColumnExtras`]: the vendor block nests under a `surreal:` key so it
/// rides in `ReferenceSpec`'s flattened `driver` slot without colliding with the
/// uniform `table`/`kind`/`foreign_key` keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealReferenceExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealRefBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealRefBlock {
    /// Rhai script that builds the traversal target for this reference, instead
    /// of the default foreign-key eq-condition path. Evaluated lazily at
    /// traversal time with the parent `row` in scope. Requires the `rhai`
    /// feature; ignored by builds without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
}

pub type SurrealVistaSpec =
    VistaSpec<SurrealTableExtras, SurrealColumnExtras, SurrealReferenceExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_surreal_vista_spec() {
        let yaml = r#"
name: client
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  is_paying_client:
    type: bool
surreal:
  table: clients
"#;
        let spec: SurrealVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "client");
        assert_eq!(spec.columns.len(), 3);
        assert_eq!(
            spec.driver
                .surreal
                .as_ref()
                .and_then(|m| m.table.as_deref()),
            Some("clients")
        );
        assert_eq!(spec.columns["id"].col_type.as_deref(), Some("thing"));
        assert!(spec.columns["name"].flags.contains(&"title".to_string()));
    }

    #[test]
    fn yaml_rejects_unknown_surreal_block_field() {
        let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
surreal:
  table: clients
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<SurrealVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }

    #[test]
    fn yaml_table_defaults_to_spec_name_when_block_omitted() {
        let yaml = r#"
name: clients
columns:
  id: { type: thing, flags: [id] }
"#;
        let spec: SurrealVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.driver.surreal.is_none());
    }

    #[test]
    fn yaml_parses_base_inherit_and_rhai() {
        let yaml = r#"
name: vip_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
    relations: [orders]
  rhai: |
    base.where(expr("name = 'Alice'"))
"#;
        let spec: SurrealVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let block = spec.driver.surreal.as_ref().expect("surreal block");
        assert_eq!(block.base.as_deref(), Some("client"));
        assert!(block.rhai.as_deref().unwrap().contains("base.where"));
        let inherit = block.inherit.as_ref().expect("inherit block");
        assert_eq!(inherit.columns, vec!["id", "name"]);
        assert_eq!(inherit.relations, vec!["orders"]);
    }

    #[test]
    fn yaml_parses_references_block() {
        let yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  clients:
    table: client
    kind: has_many
    foreign_key: bakery
  primary_product:
    table: product
    kind: has_one
    foreign_key: primary_product
"#;
        let spec: SurrealVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.references.len(), 2);

        let clients = &spec.references["clients"];
        assert_eq!(clients.table, "client");
        assert_eq!(clients.kind, vantage_vista::ReferenceKind::HasMany);
        assert_eq!(clients.foreign_key.as_deref(), Some("bakery"));

        let primary = &spec.references["primary_product"];
        assert_eq!(primary.table, "product");
        assert_eq!(primary.kind, vantage_vista::ReferenceKind::HasOne);
    }
}
