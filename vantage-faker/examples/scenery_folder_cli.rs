//! Three-pane reactive view of a live folder tree, driven by Diorama.
//!
//! Builds one Dio for the `yyyy-mm-dd` listing, then traverses into
//! `error_logs` and `events` via `Dio::get_ref("subdir", row)` — the same
//! pattern a UI detail tab uses to drill into a child folder. Each pane is a
//! `TableScenery` that re-lists from the live tree on every sim tick
//! (`ChangeEvent::Invalidated` → `dio.refresh()` → scenery reseeds →
//! redraws).
//!
//! ```sh
//! cargo run --example scenery_folder_cli
//! ```
//! Ctrl-C to quit.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tempfile::TempDir;
use vantage_cli_util::render_records;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, Dio, Lens, SortDir, TableScenery};
use vantage_faker::{LiveFolderConfig, LiveFolderSim};
use vantage_types::Record;

const VIEWPORT: usize = 30;

/// Render one scenery as a titled block. Reads up to `VIEWPORT` rows from
/// the scenery's sparse map; falls back to "(empty)" when the folder has no
/// children yet.
fn render_pane(title: &str, hint: &str, scenery: &Arc<dyn TableScenery>) {
    println!("┌─ {title} ─┐");
    if hint.is_empty() {
        println!("│ (not opened yet)");
        println!("└──────────────────────────────────────────────────────────────");
        return;
    }
    let total = scenery.row_count();
    let shown = total.min(VIEWPORT);
    println!("│ {hint} · showing {shown} of {total} rows");

    let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
    for i in 0..shown {
        if let Some(row) = scenery.row(i) {
            let id = match row.record.get("name") {
                Some(CborValue::Text(s)) => s.clone(),
                _ => i.to_string(),
            };
            records.insert(id, row.record.clone());
        }
    }
    if records.is_empty() {
        println!("│ (no rows yet this tick)");
    } else {
        // Render below the title rule. `render_records` prints its own table.
        render_records(&records, Some("name"));
    }
    println!();
}

/// Open a TableScenery on `dio`, sorted by `modified` desc so the newest
/// child floats to the top of the pane.
async fn open_scenery(dio: &vantage_diorama::Dio) -> Result<Arc<dyn TableScenery>> {
    let scenery: Arc<dyn TableScenery> = dio
        .table_scenery()
        .sort("modified", SortDir::Desc)
        .open()
        .await?;
    scenery.set_viewport(0..VIEWPORT);
    Ok(scenery)
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    // Poll-style lens: on each Invalidated, re-list the master into the cache
    // and let the scenery reseed. The FolderListingShell reads the live tree,
    // so a refresh always picks up the latest state.
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .on_refresh(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().clear().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .on_event(|dio, evt| {
                let dio = dio.clone();
                async move {
                    if matches!(evt, ChangeEvent::Invalidated) {
                        dio.refresh().await?;
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    // Sim: 2-hour backfill, 5 GB chunks, 0.1% error rate (matches `live_folder_cli`).
    let sim = LiveFolderSim::new(LiveFolderConfig {
        backfill: Duration::from_secs(2 * 3600),
        chunk_threshold: 5 * 1024 * 1024 * 1024,
        error_pct_per_sec: 0.1,
        ..LiveFolderConfig::default()
    });

    // Today's date — used as the path for the ymd listing.
    let today = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86400;
        let z = days as i64 + 719468;
        let era = z.div_euclid(146097);
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    };

    // 1. ymd Dio over the today listing.
    let (ymd_vista, events) = sim.listing_vista("ymd", &today);
    let ymd_dio = lens.make_dio(ymd_vista).await?;

    // Forward the sim's broadcast into the ymd Dio. The two child Dios share
    // the same Lens, but each needs its own forwarder — for this demo we run
    // all three off the ymd Dio's invalidate bus by refreshing every Dio on
    // every event. (The lens callback only fires for the Dio whose
    // `handle_event` was called, so we wire three forwarders below.)
    let mut rx = events.subscribe();

    // 2. Traverse into `error_logs` and `events` via get_ref("subdir", row).
    //    Find each child row in the ymd listing, then call get_ref with it.
    let error_logs_dio = match find_child_row(&ymd_dio, "error_logs").await? {
        Some(row) => Some(ymd_dio.get_ref("subdir", &row).await?),
        None => None,
    };
    let events_dio = match find_child_row(&ymd_dio, "events").await? {
        Some(row) => Some(ymd_dio.get_ref("subdir", &row).await?),
        None => None,
    };

    // 3. Open the three sceneries.
    let ymd_scenery = open_scenery(&ymd_dio).await?;
    let error_logs_scenery = match &error_logs_dio {
        Some(d) => Some(open_scenery(d).await?),
        None => None,
    };
    let events_scenery = match &events_dio {
        Some(d) => Some(open_scenery(d).await?),
        None => None,
    };

    // Forwarders: each Dio needs its own `handle_event` invocation when the
    // sim ticks. Fan the single broadcast out to all three.
    let dios_for_forward: Vec<Option<Dio>> = vec![
        Some(ymd_dio.clone()),
        error_logs_dio.clone(),
        events_dio.clone(),
    ];
    tokio::spawn(async move {
        while let Ok(evt) = rx.recv().await {
            for dio in dios_for_forward.iter().flatten() {
                let _ = dio.handle_event(evt.clone()).await;
            }
        }
    });

    println!("vantage-faker · scenery_folder_cli · Ctrl-C to quit\n");
    println!("Opening 3 sceneries via Dio::get_ref(\"subdir\", row):");
    println!("  • ymd     = listing of {today}/");
    println!(
        "  • errors  = {}",
        match &error_logs_dio {
            Some(_) => "get_ref(\"subdir\", error_logs_row)",
            None => "(no error_logs folder yet)",
        }
    );
    println!(
        "  • events  = {}",
        match &events_dio {
            Some(_) => "get_ref(\"subdir\", events_row)",
            None => "(no events folder yet)",
        }
    );
    println!();

    // Render once a second — matches the sim's tick cadence.
    loop {
        print!("\x1B[2J\x1B[H");
        println!("vantage-faker · scenery_folder_cli · Ctrl-C to quit\n");

        render_pane(
            &format!("ymd: {today}/"),
            " Dio over listing_vista(\"ymd\", today)",
            &ymd_scenery,
        );

        match &error_logs_scenery {
            Some(s) => render_pane(
                "error_logs (via get_ref subdir)",
                " Dio::get_ref(\"subdir\", error_logs_row)",
                s,
            ),
            None => render_pane("error_logs", "", &ymd_scenery),
        }

        match &events_scenery {
            Some(s) => render_pane(
                "events (via get_ref subdir)",
                " Dio::get_ref(\"subdir\", events_row)",
                s,
            ),
            None => render_pane("events", "", &ymd_scenery),
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Pull one row out of a Dio's master listing by its `name` field. Used to
/// grab the `error_logs` / `events` row so we can hand the full record to
/// `Dio::get_ref("subdir", row)`.
async fn find_child_row(dio: &Dio, name: &str) -> Result<Option<Record<CborValue>>> {
    let rows = dio.master().list_values().await?;
    Ok(rows
        .into_iter()
        .find(|(_, r)| r.get("name").and_then(|v| v.as_text()) == Some(name))
        .map(|(_, r)| r))
}
