//! YAML-facing types for the MongoDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

/// Table-level `mongo:` block in YAML. All fields optional — when the block
/// is omitted entirely, the spec name doubles as the collection name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mongo: Option<MongoBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoBlock {
    /// Override for the MongoDB collection name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mongo: Option<MongoColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoColumnBlock {
    /// BSON field name when it differs from the spec column name.
    /// Single-level rename only — for nested documents use `nested_path`.
    /// Mutually exclusive with `nested_path`; `nested_path` wins if both set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Dotted BSON path into a nested document (e.g. `address.city`). The
    /// vista surfaces the value at this path under the spec column name. On
    /// write, intermediate sub-documents are reconstructed and merged across
    /// sibling columns. On filter, the dotted form is used directly so Mongo
    /// can index the lookup server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_path: Option<String>,
}

impl MongoColumnBlock {
    /// Resolve the BSON path for this column. `nested_path` (split on `.`)
    /// wins; otherwise `field`; otherwise an empty path so the caller can fall
    /// back to the spec column name.
    pub fn resolved_path(&self) -> Option<Vec<String>> {
        if let Some(p) = &self.nested_path {
            return Some(p.split('.').map(str::to_string).collect());
        }
        if let Some(f) = &self.field {
            return Some(vec![f.clone()]);
        }
        None
    }
}

pub type MongoVistaSpec = VistaSpec<MongoTableExtras, MongoColumnExtras, NoExtras>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_parses_into_mongo_vista_spec() {
        let yaml = r#"
name: client
columns:
  _id:
    type: object_id
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  is_paying_client:
    type: bool
mongo:
  collection: clients
"#;
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.name, "client");
        assert_eq!(spec.columns.len(), 3);
        assert_eq!(
            spec.driver
                .mongo
                .as_ref()
                .and_then(|m| m.collection.as_deref()),
            Some("clients")
        );
        assert_eq!(spec.columns["_id"].col_type.as_deref(), Some("object_id"));
        assert!(spec.columns["name"].flags.contains(&"title".to_string()));
    }

    #[test]
    fn yaml_rejects_unknown_mongo_block_field() {
        let yaml = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
mongo:
  collection: clients
  bogus: 1
"#;
        let err = serde_yaml_ng::from_str::<MongoVistaSpec>(yaml).unwrap_err();
        assert!(err.to_string().contains("bogus") || err.to_string().contains("unknown"));
    }

    #[test]
    fn yaml_collection_defaults_to_spec_name_when_block_omitted() {
        let yaml = r#"
name: clients
columns:
  _id: { type: object_id, flags: [id] }
"#;
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.driver.mongo.is_none());
    }
}
