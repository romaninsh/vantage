//! YAML-facing types for the PostgreSQL Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresTableBlock {
    /// Override for the PostgreSQL table name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresColumnBlock {
    /// SQL column name when it differs from the spec column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

pub type PostgresVistaSpec = VistaSpec<PostgresTableExtras, PostgresColumnExtras, NoExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_postgres_vista_spec() {
        let yaml = r#"
name: client
columns:
  id:
    type: int
    flags: [id]
  name:
    type: string
    flags: [title]
postgres:
  table: clients
"#;
        let spec: PostgresVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "client");
        assert_eq!(
            spec.driver
                .postgres
                .as_ref()
                .and_then(|m| m.table.as_deref()),
            Some("clients")
        );
    }

    #[test]
    fn yaml_rejects_unknown_postgres_block_field() {
        let yaml = r#"
name: client
columns:
  id: { type: int, flags: [id] }
postgres:
  table: clients
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<PostgresVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }
}
