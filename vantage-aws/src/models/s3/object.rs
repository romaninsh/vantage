use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::AwsDateTime;
use crate::{AwsAccount, eq};

/// One S3 object from `ListObjectsV2`. Field names match the wire XML
/// (`<Contents><Key/><Size/>…</Contents>`) — we surface them as-is so
/// existing S3 docs translate directly. `Size` arrives as a numeric
/// string in v0 (no XML-to-typed coercion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Size", default)]
    pub size: String,
    #[serde(rename = "LastModified", default)]
    pub last_modified: String,
    #[serde(rename = "ETag", default)]
    pub etag: String,
    #[serde(rename = "StorageClass", default)]
    pub storage_class: String,
}

/// `ListObjectsV2` table. Requires `eq("Bucket", "...")` — without it
/// the path placeholder errors out at request-build time. Optional
/// `prefix` / `delimiter` / `max-keys` filters become query params if
/// supplied.
///
/// Used as the `objects` relation on [`super::bucket::Bucket`] —
/// traversing from a single bucket fills `Bucket` automatically.
pub fn objects_table(aws: AwsAccount) -> Table<AwsAccount, Object> {
    Table::new("restxml/Contents:s3/GET /{Bucket}?list-type=2", aws)
        .with_id_column("Key")
        .with_title_column_of::<String>("Size")
        .with_title_column_of::<AwsDateTime>("LastModified")
        .with_column_of::<String>("ETag")
        .with_column_of::<String>("StorageClass")
}

impl Object {
    /// Build an [`objects_table`] narrowed to the object named in
    /// `arn`. Accepts ARNs of the shape `arn:aws:s3:::<bucket>/<key>` —
    /// the bucket fills the path placeholder, the key narrows
    /// `prefix` so we only fetch the matching object.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, Object>> {
        let rest = arn.strip_prefix("arn:aws:s3:::")?;
        let (bucket, key) = rest.split_once('/')?;
        if bucket.is_empty() || key.is_empty() {
            return None;
        }
        let mut t = objects_table(aws);
        t.add_condition(eq("Bucket", bucket.to_string()));
        t.add_condition(eq("prefix", key.to_string()));
        // The post-hoc client filter at `impls::table_source` will
        // narrow on `Key` once the response comes back — `prefix` only
        // narrows server-side, so we add the literal Key match too.
        t.add_condition(eq("Key", key.to_string()));
        Some(t)
    }
}
