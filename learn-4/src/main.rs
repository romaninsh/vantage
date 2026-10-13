mod files;

use std::time::Instant;

use files::File;
use vantage_aws::prelude::*;
use vantage_diorama::prelude::*;
use vantage_vista::prelude::*;

const CACHE_FILE: &str = "cache.redb";
const BUCKET: &str = "noaa-ghcn-pds";
const PREFIX: &str = "csv/by_station/GM";
/// Keys per listing request; S3 returns at most 1000.
const PAGE_SIZE: usize = 1000;

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

    // A fresh cache file makes the eager start list the bucket from the top.
    if std::env::args().any(|a| a == "--invalidate") {
        remove_cache()?;
    }

    let lens = Lens::builder()
        .cache(Cache::redb(CACHE_FILE))
        .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }))
        .build()
        .context("Failed to build lens")?;
    let dio = lens.dio(master).build().await?;

    let start = Instant::now();
    let listing = dio.cached().list_values().await?;
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

fn remove_cache() -> VantageResult<()> {
    match std::fs::remove_file(CACHE_FILE) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).context("Failed to remove the cache")
        }
        _ => Ok(()),
    }
}

/// Pump the master listing into the cache, one page per request. S3's
/// paging cursor is simply "the last key seen" — so the last key already
/// in the cache resumes the listing, and pages loaded by an earlier run
/// (even one that was interrupted) are never fetched again.
async fn sync(cx: &LoaderCx) -> VantageResult<()> {
    let mut token: Option<CborValue> = cx
        .cache()
        .list_values()
        .await?
        .keys()
        .last()
        .cloned()
        .map(Into::into);
    loop {
        let start = Instant::now();
        let (page, next) = cx.master().fetch_cursor(token, PAGE_SIZE).await?;
        let count = page.rows.len();
        cx.cache()
            .insert_values(page.rows.into_iter().collect())
            .await?;
        println!("fetched {count} files in {:?}", start.elapsed());
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
