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
            // Don't wait for the sync: the UI opens on whatever the cache
            // holds (nothing, on a first run) and rows stream in behind it.
            .on_start_blocking(false)
            // The scenery's list pages come straight from the warmed cache —
            // zero network. The master is only contacted by the refresh
            // reconcile and the per-row detail fetches.
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
            // Reconcile against the bucket once a minute: new files appear
            // as un-hydrated rows, vanished ones drop out, changed ones are
            // demoted for re-hydration.
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

/// Same page-by-page sync as learn-4 (resuming from the last cached key),
/// except each landed page is announced on the bus — open sceneries re-list
/// and the table grows under the user's cursor.
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
        let (page, next) = dio.master().fetch_next(token).await?;
        dio.cache()
            .insert_values(page.into_iter().collect())
            .await?;
        dio.notify_dataset_changed();
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
