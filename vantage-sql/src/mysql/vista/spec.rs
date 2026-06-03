//! YAML-facing types for the MySQL Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlTableBlock {
    /// Override for the MySQL table name. Defaults to the spec name.
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
pub struct MysqlColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlColumnBlock {
    /// SQL column name when it differs from the spec column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

pub type MysqlVistaSpec = VistaSpec<MysqlTableExtras, MysqlColumnExtras, NoExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_mysql_vista_spec() {
        let yaml = r#"
name: client
columns:
  id:
    type: int
    flags: [id]
  name:
    type: string
    flags: [title]
mysql:
  table: clients
"#;
        let spec: MysqlVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "client");
        assert_eq!(
            spec.driver.mysql.as_ref().and_then(|m| m.table.as_deref()),
            Some("clients")
        );
    }

    #[test]
    fn yaml_rejects_unknown_mysql_block_field() {
        let yaml = r#"
name: client
columns:
  id: { type: int, flags: [id] }
mysql:
  table: clients
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<MysqlVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }

    #[test]
    fn yaml_parses_base_inherit_and_rhai() {
        let yaml = r#"
name: vip_clients
columns: {}
mysql:
  base: client
  inherit:
    columns: [id, name]
    relations: [orders]
  rhai: |
    base.where(expr("vip = true"))
"#;
        let spec: MysqlVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let block = spec.driver.mysql.as_ref().unwrap();
        assert_eq!(block.base.as_deref(), Some("client"));
        assert!(block.rhai.as_ref().unwrap().contains("base.where"));
        let inherit = block.inherit.as_ref().unwrap();
        assert_eq!(inherit.columns, vec!["id", "name"]);
        assert_eq!(inherit.relations, vec!["orders"]);
    }
}
