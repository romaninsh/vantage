mod files;

use std::sync::Arc;
use std::time::Instant;

use files::File;
use vantage_aws::prelude::*;
use vantage_diorama::prelude::*;
use vantage_vista::prelude::*;

const BUCKET: &str = "noaa-ghcn-pds";
const PREFIX: &str = "csv/by_station/GM";

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let aws = AwsAccount::public("us-east-1");
    let master = aws
        .vista_factory()
        .from_table(File::table(aws, BUCKET, PREFIX))?;

    let lens = Arc::new(
        Lens::new()
            .cache_at("cache.redb")
            .on_start(|dio| {
                let dio = dio.clone();
                async move { sync(&dio).await }
            })
            .build()
            .context("Failed to build lens")?,
    );
    let dio = lens.make_dio(master).await?;

    if std::env::args().any(|a| a == "--invalidate") {
        dio.cache().clear().await?;
        sync(&dio).await?;
    }

    let start = Instant::now();
    let listing = dio.vista().list_values().await?;
    for (filename, file) in &listing {
        let size = file.get("Size").and_then(|v| v.as_str()).unwrap_or("");
        println!("{size:>10}  {filename}");
    }
    println!(
        "{} files from cache in {:?}",
        listing.len(),
        start.elapsed()
    );
    Ok(())
}

/// Pump the master listing into the cache, one page per request. S3's
/// paging cursor is simply "the last key seen" — so the last key already
/// in the cache resumes the listing, and pages loaded by an earlier run
/// (even one that was interrupted) are never fetched again.
async fn sync(dio: &Dio) -> VantageResult<()> {
    let mut token: Option<CborValue> = dio
        .cache()
        .list_values()
        .await?
        .keys()
        .last()
        .cloned()
        .map(Into::into);
    loop {
        let start = Instant::now();
        let (page, next) = dio.master().fetch_next(token).await?;
        let count = page.len();
        dio.cache()
            .insert_values(page.into_iter().collect())
            .await?;
        println!("fetched {count} files in {:?}", start.elapsed());
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
