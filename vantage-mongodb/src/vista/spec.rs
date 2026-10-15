//! YAML-facing types for the MongoDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_vista::VistaSpec;

pub(crate) mod pipeline;

pub use pipeline::DriverBlockArgs;

/// Table-level driver slot: the optional `mongo:` block. Without it the spec
/// name doubles as the collection name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mongo: Option<MongoBlock>,
}

/// A table's `mongo:` block: which collection it reads, whether that is a
/// view or a pipeline, and the Rhai scripts that shape it.
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
    /// `sortBy`) must be built with `mongo::doc([["b", 1], ["a", -1]])`, and an
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

/// Column-level driver slot: the optional `mongo:` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mongo: Option<MongoColumnBlock>,
}

/// A column's `mongo:` block: where its value lives in the document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoColumnBlock {
    /// BSON field name when it differs from the spec column name.
    /// Single-level rename only; for nested documents use `nested_path`.
    /// `nested_path` wins if both are set. A reference key cannot be
    /// renamed; see `nested_path`.
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
    /// The BSON path for this column: `nested_path` split on `.`, else
    /// `field`, else `Ok(None)` so the caller falls back to the spec column
    /// name.
    ///
    /// An empty `field`, or a `nested_path` that is empty or has an empty
    /// segment (`""`, `".a"`, `"a..b"`), is an error naming `column_name`.
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

/// A reference's `mongo:` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoRefBlock {
    /// Rhai script that builds the traversal target for this reference,
    /// instead of the foreign-key condition, e.g.
    /// `table("order").where("client_id", row._id)`. Evaluated at traversal
    /// time with the parent row in scope as `row` (and `me`). Requires the
    /// `rhai` feature: a build without it fails on a spec that sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
}

pub type MongoVistaSpec = VistaSpec<MongoTableExtras, MongoColumnExtras, MongoReferenceExtras>;
