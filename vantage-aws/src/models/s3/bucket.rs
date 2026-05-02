use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::AwsDateTime;
use crate::{AwsAccount, eq};

use super::object::{Object, objects_table};

/// One S3 bucket from `ListBuckets`. The wire shape is
/// `<Bucket><Name>...</Name><CreationDate>...</CreationDate></Bucket>`
/// inside `<Buckets>` — we surface those two fields verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CreationDate", default)]
    pub creation_date: String,
}

/// `ListBuckets` table — every bucket the caller can see. S3 returns
/// buckets across all regions here, but cross-region object listings
/// require the bucket's home region — the caller is expected to set
/// `AwsAccount`'s region accordingly before traversing `:objects`.
///
/// Relation:
///   - `objects` → `ListObjectsV2` for the bucket
///
/// ```no_run
/// # use vantage_aws::AwsAccount;
/// # use vantage_aws::models::s3::buckets_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let buckets = buckets_table(aws);
/// # Ok(()) }
/// ```
pub fn buckets_table(aws: AwsAccount) -> Table<AwsAccount, Bucket> {
    Table::new("restxml/Buckets.Bucket:s3/GET /", aws)
        .with_id_column("Name")
        .with_title_column_of::<AwsDateTime>("CreationDate")
        .with_many("objects", "Bucket", objects_table)
}

impl Bucket {
    /// Build a [`buckets_table`] narrowed to the bucket named in `arn`.
    /// Accepts ARNs of the shape `arn:aws:s3:::<name>` (S3 ARNs have
    /// no region or account segment — that's the protocol's quirk,
    /// not a bug here).
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, Bucket>> {
        let name = arn.strip_prefix("arn:aws:s3:::")?;
        // Object-level ARNs (`arn:aws:s3:::bucket/key`) collapse to the
        // bucket; `Object::from_arn` handles the object-level case.
        let bucket = name.split('/').next().unwrap_or(name);
        if bucket.is_empty() {
            return None;
        }
        let mut t = buckets_table(aws);
        t.add_condition(eq("Name", bucket.to_string()));
        Some(t)
    }

    /// Objects table pre-filtered to *this* bucket.
    pub fn ref_objects(&self, aws: AwsAccount) -> Table<AwsAccount, Object> {
        let mut t = objects_table(aws);
        t.add_condition(eq("Bucket", self.name.clone()));
        t
    }
}
