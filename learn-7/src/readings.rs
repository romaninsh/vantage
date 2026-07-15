use std::sync::Arc;

use serde::{Deserialize, Serialize};
use vantage_aws::prelude::*;

use crate::contents::ContentsCache;

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
    /// declaration order — `contents` produces the file's body once, and
    /// the columns declared after it derive their values from it. The
    /// download goes through the [`ContentsCache`], so a file the API has
    /// served repeatedly is read from disk instead of S3. No prefix here:
    /// the augmentation narrows this table to a single key per fetch.
    pub fn table(
        aws: AwsAccount,
        bucket: &str,
        contents: Arc<ContentsCache>,
    ) -> Table<AwsAccount, Readings> {
        let bucket = bucket.to_string();
        Table::new("restxml/Contents:s3/GET /{Bucket}?list-type=2", aws.clone())
            .with_id_column("Key")
            .with_condition(eq("Bucket", bucket.clone()))
            .with_lazy_expression("contents", move |row| {
                let aws = aws.clone();
                let bucket = bucket.clone();
                let contents = contents.clone();
                let key = row.get("Key").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                async move {
                    let fetch_key = key.clone();
                    let body = contents
                        .get_or_fetch(&key, || async move {
                            s3::get_object(&aws, &bucket, &fetch_key).await
                        })
                        .await?;
                    Ok(body.into())
                }
            })
            .with_lazy_expression("rows", |row| {
                // Every line after the CSV header is one reading.
                let contents = row.get("contents").and_then(|v| v.as_str()).unwrap_or_default();
                let rows = contents.lines().count().saturating_sub(1) as i64;
                async move { Ok(rows.into()) }
            })
            .with_lazy_expression("latest", |row| {
                // Readings are date-ordered; take the last line's DATE column.
                let contents = row.get("contents").and_then(|v| v.as_str()).unwrap_or_default();
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
