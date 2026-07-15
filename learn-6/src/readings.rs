use serde::{Deserialize, Serialize};
use vantage_aws::prelude::*;

/// One file, seen through the augmenter: the listing row plus columns
/// computed from the file's contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Readings {
    #[serde(rename = "Key")]
    pub filename: String,
    pub rows: i64,
    pub latest: String,
}

impl Readings {
    /// The augmenter table: the same `ListObjectsV2` listing, extended
    /// with lazy columns. Lazy expressions run on the returned record in
    /// declaration order — `contents` downloads the file once, and the
    /// columns declared after it derive their values from it. No prefix
    /// here: the augmentation narrows this table to a single key per
    /// fetch.
    pub fn table(aws: AwsAccount, bucket: &str) -> Table<AwsAccount, Readings> {
        let bucket = bucket.to_string();
        Table::new("restxml/Contents:s3/GET /{Bucket}?list-type=2", aws.clone())
            .with_id_column("Key")
            .with_condition(eq("Bucket", bucket.clone()))
            .with_lazy_expression("contents", move |row| {
                let aws = aws.clone();
                let bucket = bucket.clone();
                let key = row
                    .get("Key")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                async move { Ok(s3::get_object(&aws, &bucket, &key).await?.into()) }
            })
            .with_lazy_expression("rows", |row| {
                // Every line after the CSV header is one reading.
                let contents = row
                    .get("contents")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let rows = contents.lines().count().saturating_sub(1) as i64;
                async move { Ok(rows.into()) }
            })
            .with_lazy_expression("latest", |row| {
                // Readings are date-ordered; take the last line's DATE column.
                let contents = row
                    .get("contents")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let latest = contents
                    .lines()
                    .last()
                    .and_then(|line| line.split(',').nth(1))
                    .unwrap_or_default()
                    .to_string();
                async move { Ok(latest.into()) }
            })
    }
}
