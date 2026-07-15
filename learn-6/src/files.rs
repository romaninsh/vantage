use serde::{Deserialize, Serialize};
use vantage_aws::prelude::*;

/// One file in the bucket. Field names match S3's wire XML
/// (`<Contents><Key/><Size/></Contents>`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct File {
    #[serde(rename = "Key")]
    pub filename: String,
    #[serde(rename = "Size")]
    pub size: String,
}

impl File {
    /// `ListObjectsV2` narrowed to one bucket and prefix. S3 sends at
    /// most `max-keys` keys per response; the `@continuation-token`
    /// cursor tells the framework to keep requesting pages until the
    /// listing is complete.
    pub fn table(aws: AwsAccount, bucket: &str, prefix: &str) -> Table<AwsAccount, File> {
        Table::new(
            "restxml/Contents@continuation-token=NextContinuationToken:s3/GET /{Bucket}?list-type=2",
            aws,
        )
        .with_id_column("Key")
        .with_column_of::<String>("Size")
        .with_condition(eq("Bucket", bucket))
        .with_condition(eq("prefix", prefix))
        // S3's per-response maximum — we're listing the whole station set.
        .with_condition(eq("max-keys", 1000))
    }
}
