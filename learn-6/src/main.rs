mod files;
mod readings;

use std::sync::Arc;
use std::time::Duration;

use dataset_ui_adapters::ratatui_dio;
use files::File;
use readings::Readings;
use vantage_aws::prelude::*;
use vantage_diorama::prelude::*;
use vantage_vista::prelude::*;

const BUCKET: &str = "noaa-ghcn-pds";
const PREFIX: &str = "csv/by_station/";
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

    let lens = Lens::builder()
        .cache(Cache::redb("cache.redb"))
        // Don't wait for the sync: it warms the cache for later runs in the
        // background, while the view lists the bucket itself.
        .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }).blocking(false))
        // Reconcile against the bucket once a minute: new files appear
        // as un-hydrated rows, vanished ones drop out, changed ones are
        // demoted for re-hydration. The augmentation's refresh pass does
        // the work; the refresher only sets the interval.
        .refresher(Refresher::new(|_cx: LoaderCx| async { Ok(()) }).every(Duration::from_secs(60)))
        .build()
        .context("Failed to build lens")?;

    let dio = lens
        .dio(master)
        .augment(vec![Augmentation {
            detail: Detail::Fixed(Arc::new(augmenter)),
            source: Source::Column {
                from: "Key".into(),
                to: Some("prefix".into()),
            },
            fetch: Fetch::PerRow,
            merge: MergeRule {
                columns: vec!["rows".into(), "latest".into()],
            },
        }])
        .build()
        .await?;

    // One list page covers the whole cached listing (~122k station files);
    // the viewport's detail pass hydrates whatever is on screen first.
    let scenery = dio.table_scenery().page_size(200_000).open().await?;

    // Grand total of the ROWS column — recomputes reactively as files
    // hydrate, so the status bar counts up while data arrives.
    let totals = dio.value_scenery().sum("rows").open().await?;

    ratatui_dio::SceneryTable::new(scenery)
        .with_column("FILENAME", "Key", 0)
        .with_column("SIZE", "Size", 10)
        .with_column("ROWS", "rows", 8)
        .with_column("LATEST", "latest", 10)
        .with_status_value("total rows", totals)
        .run()
        .await
        .context("terminal UI failed")
}

/// Same page-by-page sync as learn-4, resuming from the last cached key.
/// The Dio announces the rows once the detached sync finishes.
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
        let (page, next) = cx.master().fetch_cursor(token, PAGE_SIZE).await?;
        cx.cache()
            .insert_values(page.rows.into_iter().collect())
            .await?;
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
