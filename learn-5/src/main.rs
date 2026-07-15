mod files;
mod readings;

use std::sync::Arc;
use std::time::Instant;

use files::File;
use readings::Readings;
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
        .from_table(File::table(aws.clone(), BUCKET, PREFIX))?;
    let augmenter = aws
        .vista_factory()
        .from_table(Readings::table(aws.clone(), BUCKET))?;

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

    // Every listed file gains `rows` and `latest`, computed from its
    // contents by the augmenter. `prefix` narrows the augmenter's listing
    // to exactly one key per row.
    let dio = lens.make_dio(master).await?.augment(
        Arc::new(VistaCatalog::new()),
        vec![Augmentation {
            detail: Detail::Fixed(Arc::new(augmenter)),
            source: Source::Column {
                from: "Key".into(),
                to: Some("prefix".into()),
            },
            fetch: Fetch::PerRow,
            merge: MergeRule {
                columns: vec!["rows".into(), "latest".into()],
            },
        }],
    );

    if std::env::args().any(|a| a == "--invalidate") {
        dio.cache().clear().await?;
        sync(&dio).await?;
    }

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
    let listing = dio.vista().list_values().await?;
    println!("{} files (listed in {:?})", listing.len(), start.elapsed());

    // Details are paid for by the rows you ask for: a window of ten.
    let start = Instant::now();
    let window = dio.vista().fetch_window(0, 10).await?;
    for (filename, file) in &window {
        let size = file.get("Size").and_then(|v| v.as_str()).unwrap_or("");
        let rows = file.get("rows").and_then(|v| v.as_i64()).unwrap_or(0);
        let latest = file.get("latest").and_then(|v| v.as_str()).unwrap_or("");
        println!("{size:>10} {rows:>8} {latest:>10}  {filename}");
    }
    println!("{} files detailed in {:?}", window.len(), start.elapsed());
    Ok(())
}

/// Same sync as learn-4: pump the listing into the cache one page per
/// request, resuming from the last cached key.
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
        dio.cache().insert_values(page.into_iter().collect()).await?;
        println!("fetched {count} files in {:?}", start.elapsed());
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
