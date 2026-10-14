mod files;
mod readings;

use std::sync::Arc;
use std::time::Instant;

use files::File;
use readings::Readings;
use vantage_aws::prelude::*;
use vantage_diorama::prelude::*;
use vantage_diorama::{
    Augmentation, Cache, Detail, DioEvent, Fetch, Loader, LoaderCx, MergeRule, Source,
};
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
        .from_table(File::table(aws.clone(), BUCKET, PREFIX))?;
    let augmenter = aws
        .vista_factory()
        .from_table(Readings::table(aws.clone(), BUCKET))?;

    // A fresh cache file makes the eager start list the bucket from the top.
    if std::env::args().any(|a| a == "--invalidate") {
        remove_cache()?;
    }

    let lens = Lens::builder()
        .cache(Cache::redb(CACHE_FILE))
        .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }))
        .build()
        .context("Failed to build lens")?;

    // Every listed file gains `rows` and `latest`, computed from its
    // contents by the augmenter. `prefix` narrows the augmenter's listing
    // to exactly one key per row.
    let dio = lens
        .dio(master)
        .augment(vec![Augmentation::new(
            Detail::Fixed(Arc::new(augmenter)),
            Source::Column {
                from: "Key".into(),
                to: Some("prefix".into()),
            },
            Fetch::PerRow,
            MergeRule::lift(["rows", "latest"]),
        )])
        .build()
        .await?;

    // A facade read announces its hydration sweep before the first fetch,
    // then reports each hydrated row — our progress display.
    let mut events = dio.subscribe_events();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event {
                DioEvent::Hydrating { pending } => println!("hydrating {pending} files…"),
                DioEvent::RecordChanged { id } => println!("  {id}"),
                _ => {}
            }
        }
    });

    // The listing stays instant — cheap rows, no downloads.
    let start = Instant::now();
    let listing = dio.cached().list_values().await?;
    println!("{} files (listed in {:?})", listing.len(), start.elapsed());

    // Details are paid for by the rows you ask for: a window of ten.
    let start = Instant::now();
    let window = dio.cached().fetch_window(0, 10).await?;
    for (filename, file) in &window.rows {
        let size = file.get("Size").and_then(|v| v.as_str()).unwrap_or("");
        let rows = file.get("rows").and_then(|v| v.as_i64()).unwrap_or(0);
        let latest = file.get("latest").and_then(|v| v.as_str()).unwrap_or("");
        println!("{size:>10} {rows:>8} {latest:>10}  {filename}");
    }
    println!(
        "{} files detailed in {:?}",
        window.rows.len(),
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

/// Same sync as learn-4: pump the listing into the cache one page per
/// request, resuming from the last cached key.
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
