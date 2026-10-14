mod contents;
mod files;
mod readings;

use std::sync::Arc;
use std::time::{Duration, Instant};

use contents::ContentsCache;
use files::File;
use readings::Readings;
use tower_http::services::ServeDir;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_aws::prelude::*;
use vantage_diorama::prelude::*;
use vantage_diorama::{
    Augmentation, Cache, CacheBackend, Detail, Fetch, Loader, LoaderCx, MergeRule, RedbCache,
    Refresher, Source,
};
use vantage_vista::prelude::*;

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

    // One redb file, two tables: the Dio's listing cache, and our contents
    // cache with its lazy admission policy.
    let cache = Arc::new(RedbCache::open("cache.redb").context("Failed to open cache")?);
    let contents = ContentsCache::new(cache.open_table("contents").await?);

    let master = aws
        .vista_factory()
        .from_table(File::table(aws.clone(), BUCKET, PREFIX))?;
    let augmenter =
        aws.vista_factory()
            .from_table(Readings::table(aws.clone(), BUCKET, contents))?;

    let lens = Lens::builder()
        .cache(Cache::backend(cache))
        // Pre-fetch: the eager start blocks the Dio's build (the default),
        // so the server starts answering only once the listing is cached. A
        // restart resumes from redb with a single confirming request.
        .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }))
        // The augmentation's refresh pass reconciles the listing once a
        // minute; the refresher only sets the interval.
        .refresher(Refresher::new(|_cx: LoaderCx| async { Ok(()) }).every(Duration::from_secs(60)))
        .build()
        .context("Failed to build lens")?;

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

    // The whole API surface: GET + watch on the listing and on each file.
    let api = DioRouter::new(dio.clone())
        .with_column("filename", "Key")
        .with_column("size", "Size")
        .with_column("rows", "rows")
        .with_column("latest", "latest")
        .with_page_size(50)
        .into_router();

    let app = axum::Router::new()
        .nest("/api/files", api)
        .fallback_service(ServeDir::new("frontend/dist"));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3007")
        .await
        .context("Failed to bind :3007")?;
    println!("serving on http://localhost:3007");
    axum::serve(listener, app).await.context("server failed")
}

/// Same page-by-page sync as learn-4: pump the listing into the cache one
/// page per request, resuming from the last cached key.
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
