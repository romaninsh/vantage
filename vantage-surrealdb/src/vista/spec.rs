//! YAML-facing types for the SurrealDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

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

pub type SurrealVistaSpec = VistaSpec<SurrealTableExtras, SurrealColumnExtras, NoExtras>;

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
}
