//! YAML-facing types for the REST API Vista driver.
//!
//! `base_url`, auth header, response shape and pagination convention stay on
//! the factory's `RestApi` — they're per-deployment, not per-table, and don't
//! belong in checked-in YAML. The per-table block only carries the endpoint
//! path when it differs from the spec name.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<RestApiTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiTableBlock {
    /// Endpoint path segment when it differs from the spec name. Joined to
    /// the factory's `RestApi.base_url` — same shape `RestApi::endpoint_url`
    /// would build for any other table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<RestApiColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiColumnBlock {
    /// JSON field name when it differs from the spec column name. Becomes the
    /// column's alias so reads/writes target the underlying field while the
    /// vista surfaces it under the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

pub type RestApiVistaSpec = VistaSpec<RestApiTableExtras, RestApiColumnExtras, NoExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_rest_api_vista_spec() {
        let yaml = r#"
name: comments
columns:
  id:
    type: int
    flags: [id]
  postId:
    type: int
  body:
    type: string
    flags: [title]
api:
  endpoint: comments
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "comments");
        assert_eq!(spec.columns.len(), 3);
        assert_eq!(
            spec.driver
                .api
                .as_ref()
                .and_then(|m| m.endpoint.as_deref()),
            Some("comments")
        );
        assert_eq!(spec.columns["id"].col_type.as_deref(), Some("int"));
        assert!(spec.columns["body"].flags.contains(&"title".to_string()));
    }

    #[test]
    fn yaml_parses_column_field_alias() {
        let yaml = r#"
name: users
columns:
  id:
    type: int
    flags: [id]
  full_name:
    type: string
    api:
      field: name
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            spec.columns["full_name"]
                .driver
                .api
                .as_ref()
                .and_then(|b| b.field.as_deref()),
            Some("name")
        );
    }

    #[test]
    fn yaml_rejects_unknown_api_block_field() {
        let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
api:
  endpoint: users
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<RestApiVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }

    #[test]
    fn yaml_endpoint_defaults_to_spec_name_when_block_omitted() {
        let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.driver.api.is_none());
    }
}
