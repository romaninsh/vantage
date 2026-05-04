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
}
