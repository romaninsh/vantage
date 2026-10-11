//! YAML-facing types for the MongoDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_vista::VistaSpec;

pub(crate) mod pipeline;

pub use pipeline::DriverBlockArgs;

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
    /// The collection is a server-side view (`db.createView`). The vista
    /// reads it and is read-only, with no live updates, import or traversal,
    /// like a `pipeline` view; a `pipeline` or `rhai` then runs over the
    /// server-side view. An undeclared view fails its first write or watch
    /// with an unsupported error naming this flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub view: bool,
    /// Aggregation stages over the collection. When set, the vista reads the
    /// pipeline's output and is read-only; its rows are keyed by the `_id`
    /// the pipeline emits (for a `$group`, the group key), which must be a
    /// string or an `ObjectId`. Keys keep the order written in YAML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Vec<serde_json::Value>>,
    /// A Rhai script that evaluates to the pipeline: an array of stage maps,
    /// e.g. `[#{"$match": #{category: args.cat}}]`. The vista is a read-only
    /// pipeline view exactly as with `pipeline`, which it excludes. Requires
    /// the `rhai` feature.
    ///
    /// Unlike YAML `pipeline:`, Rhai map literals sort their keys. That is
    /// harmless where MongoDB ignores key order (`$match`, `$group`,
    /// `$project` fields), but a multi-key `$sort` (or `$setWindowFields`'
    /// `sortBy`) must be built with `doc([["b", 1], ["a", -1]])`, and an
    /// exact match on an embedded document should use dotted paths
    /// (`#{"address.city": "Riga"}`) instead of a nested map. A missing
    /// `args.x` reads as `()`, which becomes BSON `null`; guard optional
    /// args with `"x" in args`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
    /// A Rhai script applied to the *built* vista as a final step, exposed as
    /// `self`, e.g. `self.with_condition(#{price: #{"$gt": 100}})`. Composes
    /// with `collection`/`pipeline`/`rhai` (it runs last). Requires the `rhai`
    /// feature. Filter maps sort their keys like `rhai:` stages, so match
    /// embedded documents by dotted path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<String>,
    /// Observation-supplied parameters, exposed to the `rhai` script as the
    /// `args` map (`args.cat`, …) so a view can filter before it aggregates.
    /// Set programmatically by the host at open time, never from YAML (hence
    /// no serde). A `modify:` script runs without them.
    #[serde(skip)]
    pub args: Vec<(String, String)>,
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
    /// A reference key cannot be renamed; see `nested_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Dotted BSON path into a nested document (e.g. `address.city`). The
    /// vista surfaces the value at this path under the spec column name. On
    /// write, intermediate sub-documents are reconstructed and merged across
    /// sibling columns. On filter, the dotted form is used directly so Mongo
    /// can index the lookup server-side.
    ///
    /// A reference key cannot live at a nested path (nor be renamed with
    /// `field`): building a vista fails when a reference's foreign key, or
    /// the id column it matches on the other side, is stored anywhere but a
    /// field of its own name ("reference key must be stored under its own
    /// field name"). A reference with a `mongo: { rhai: }` build script is
    /// exempt under the `rhai` feature. Joined (dotted) columns may still
    /// read nested fields of the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_path: Option<String>,
}

impl MongoColumnBlock {
    /// Resolve the BSON path for this column. `nested_path` (split on `.`)
    /// wins; otherwise `field`; otherwise `Ok(None)` so the caller falls back
    /// to the spec column name.
    ///
    /// Validates that neither `nested_path` nor `field` is empty, and that
    /// `nested_path` contains no empty segments (e.g. `""`, `".a"`, `"a..b"`).
    /// `column_name` is woven into the error so the caller can locate the bad
    /// entry in YAML without re-walking the spec.
    pub fn resolved_path(&self, column_name: &str) -> Result<Option<Vec<String>>> {
        if let Some(p) = &self.nested_path {
            let segments: Vec<String> = p.split('.').map(str::to_string).collect();
            if segments.iter().any(String::is_empty) {
                return Err(error!(
                    "nested_path must not be empty or contain empty segments",
                    column = column_name.to_string(),
                    nested_path = p.clone()
                ));
            }
            return Ok(Some(segments));
        }
        if let Some(f) = &self.field {
            if f.is_empty() {
                return Err(error!(
                    "field must not be empty",
                    column = column_name.to_string()
                ));
            }
            return Ok(Some(vec![f.clone()]));
        }
        Ok(None)
    }
}

/// Per-reference `mongo:` block, riding in `ReferenceSpec`'s flattened
/// `driver` slot beside the uniform `table`/`kind`/`foreign_key` keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoReferenceExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mongo: Option<MongoRefBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoRefBlock {
    /// Rhai script that builds the traversal target for this reference,
    /// instead of the foreign-key condition, e.g.
    /// `table("order").where("client_id", row._id)`. Evaluated at traversal
    /// time with the parent row in scope as `row` (and `me`). Requires the
    /// `rhai` feature; ignored by builds without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
}

pub type MongoVistaSpec = VistaSpec<MongoTableExtras, MongoColumnExtras, MongoReferenceExtras>;

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
    fn yaml_view_flag_parses_and_defaults_off() {
        let yaml = r#"
name: report
columns:
  _id: { type: object_id, flags: [id] }
mongo:
  collection: sales_report
  view: true
"#;
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(spec.driver.mongo.as_ref().unwrap().view);
        assert!(!MongoBlock::default().view);

        let unknown = format!("{yaml}  bogus: 1\n");
        let err = serde_yaml_ng::from_str::<MongoVistaSpec>(&unknown).unwrap_err();
        assert!(err.to_string().contains("bogus"), "{err}");
    }

    #[test]
    fn yaml_view_flag_is_omitted_when_off() {
        let off = serde_yaml_ng::to_string(&MongoBlock::default()).unwrap();
        assert!(!off.contains("view"), "{off}");
        let on = MongoBlock {
            view: true,
            ..MongoBlock::default()
        };
        assert!(
            serde_yaml_ng::to_string(&on)
                .unwrap()
                .contains("view: true")
        );
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

    #[test]
    fn yaml_pipeline_parses_into_bson_stages() {
        let yaml = r#"
name: totals
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  pipeline:
    - $group: { _id: "$category", total: { $sum: "$price" } }
    - $sort: { total: -1 }
"#;
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let stages = spec
            .driver
            .mongo
            .unwrap()
            .pipeline_stages()
            .unwrap()
            .unwrap();
        assert_eq!(
            stages,
            vec![
                bson::doc! { "$group": { "_id": "$category", "total": { "$sum": "$price" } } },
                bson::doc! { "$sort": { "total": -1_i64 } },
            ]
        );
    }

    #[test]
    fn yaml_parses_scripts_and_reference_block() {
        let yaml = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
mongo:
  rhai: '[#{"$match": #{}}]'
  modify: 'self.with_condition(#{a: 1})'
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo:
      rhai: 'table("order").where("client_id", row._id)'
"#;
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let block = spec.driver.mongo.as_ref().unwrap();
        assert!(block.rhai.as_deref().unwrap().contains("$match"));
        assert!(block.modify.as_deref().unwrap().contains("with_condition"));
        assert!(block.args.is_empty());
        let reference = spec.references["orders"].driver.mongo.as_ref().unwrap();
        assert!(reference.rhai.as_deref().unwrap().starts_with("table("));
    }

    #[test]
    fn pipeline_stages_absent_without_pipeline() {
        assert!(MongoBlock::default().pipeline_stages().unwrap().is_none());
    }

    #[test]
    fn pipeline_rejects_empty_pipeline() {
        let block = MongoBlock {
            pipeline: Some(vec![]),
            ..MongoBlock::default()
        };
        let msg = block.pipeline_stages().unwrap_err().to_string();
        assert!(msg.contains("at least one stage"), "{msg}");
    }

    #[test]
    fn pipeline_rejects_bad_stage_naming_its_index() {
        for bad in [
            serde_json::json!({ "group": { "_id": 1 } }),
            serde_json::json!({ "$match": {}, "$sort": { "a": 1 } }),
            serde_json::json!(["$match"]),
            serde_json::json!({}),
        ] {
            let block = MongoBlock {
                pipeline: Some(vec![serde_json::json!({ "$match": {} }), bad.clone()]),
                ..MongoBlock::default()
            };
            let msg = block.pipeline_stages().unwrap_err().to_string();
            assert!(msg.contains("pipeline stage"), "{bad}: {msg}");
            assert!(msg.contains("stage: 1"), "{bad}: {msg}");
        }
    }

    #[test]
    fn resolved_path_rejects_empty_nested_path() {
        let block = MongoColumnBlock {
            field: None,
            nested_path: Some(String::new()),
        };
        let err = block
            .resolved_path("city")
            .expect_err("empty nested_path must error");
        let msg = err.to_string();
        assert!(msg.contains("nested_path"), "error: {msg}");
        assert!(msg.contains("city"), "error: {msg}");
    }

    #[test]
    fn resolved_path_rejects_nested_path_with_empty_segments() {
        for bad in ["a..b", ".a", "a.", "."] {
            let block = MongoColumnBlock {
                field: None,
                nested_path: Some(bad.to_string()),
            };
            assert!(
                block.resolved_path("city").is_err(),
                "path `{bad}` should have errored"
            );
        }
    }

    #[test]
    fn resolved_path_rejects_empty_field() {
        let block = MongoColumnBlock {
            field: Some(String::new()),
            nested_path: None,
        };
        let err = block
            .resolved_path("name")
            .expect_err("empty field must error");
        let msg = err.to_string();
        assert!(msg.contains("field"), "error: {msg}");
        assert!(msg.contains("name"), "error: {msg}");
    }

    #[test]
    fn resolved_path_accepts_valid_nested_and_field() {
        let block = MongoColumnBlock {
            field: None,
            nested_path: Some("address.city".into()),
        };
        assert_eq!(
            block.resolved_path("city").unwrap(),
            Some(vec!["address".into(), "city".into()])
        );

        let block = MongoColumnBlock {
            field: Some("fullName".into()),
            nested_path: None,
        };
        assert_eq!(
            block.resolved_path("full_name").unwrap(),
            Some(vec!["fullName".into()])
        );

        let block = MongoColumnBlock::default();
        assert_eq!(block.resolved_path("anything").unwrap(), None);
    }
}
