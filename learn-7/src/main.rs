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

    let lens = Arc::new(
        Lens::new()
            .cache_source(cache)
            // Pre-fetch: on_start blocks make_dio (the default), so the
            // server starts answering only once the listing is cached. A
            // restart resumes from redb with a single confirming request.
            .on_start(|dio| {
                let dio = dio.clone();
                async move { sync(&dio).await }
            })
            // Watch sceneries list their pages straight from the warmed
            // cache — the master is only contacted by the refresh reconcile
            // and the per-row detail fetches.
            .on_list_page(|dio, q| {
                let dio = dio.clone();
                async move {
                    Ok(dio
                        .cache()
                        .list_values()
                        .await?
                        .into_iter()
                        .skip(q.offset)
                        .take(q.limit)
                        .collect())
                }
            })
            .refresh_every(Duration::from_secs(60))
            .build()
            .context("Failed to build lens")?,
    );

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
