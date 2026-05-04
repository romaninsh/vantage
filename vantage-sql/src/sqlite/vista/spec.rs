//! YAML-facing types for the SQLite Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite: Option<SqliteTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteTableBlock {
    /// Override for the SQLite table name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite: Option<SqliteColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteColumnBlock {
    /// SQL column name when it differs from the spec column name. The vista
    /// surfaces values under the spec name; on read the column is `SELECT`ed
    /// via this name and aliased back to the spec column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

pub type SqliteVistaSpec = VistaSpec<SqliteTableExtras, SqliteColumnExtras, NoExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_sqlite_vista_spec() {
        let yaml = r#"
name: client
columns:
  id:
    type: int
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  is_paying_client:
    type: bool
sqlite:
  table: clients
"#;
        let spec: SqliteVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "client");
        assert_eq!(spec.columns.len(), 3);
        assert_eq!(
            spec.driver.sqlite.as_ref().and_then(|m| m.table.as_deref()),
            Some("clients")
        );
        assert_eq!(spec.columns["id"].col_type.as_deref(), Some("int"));
        assert!(spec.columns["name"].flags.contains(&"title".to_string()));
    }

    #[test]
    fn yaml_rejects_unknown_sqlite_block_field() {
        let yaml = r#"
name: client
columns:
  id: { type: int, flags: [id] }
sqlite:
  table: clients
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<SqliteVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }

    #[test]
    fn yaml_table_defaults_to_spec_name_when_block_omitted() {
        let yaml = r#"
name: clients
columns:
  id: { type: int, flags: [id] }
"#;
        let spec: SqliteVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.driver.sqlite.is_none());
    }
}
