//! `vantage_diorama::stats`: the fetch ledger, the live-instance counts and
//! the session summary, read in Rust with no log to parse.

#![cfg(feature = "diagnostics")]

mod support;

use std::sync::Arc;
use std::time::Duration;

use vantage_diorama::stats::{
    self, debug_summary_lines, emit_debug_summary, fetch_stats, live_counts, reset_fetch_stats,
};
use vantage_diorama::{Cache, Dio, IdStrategy, Lens, TableScenery, debug};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use support::capture::{capture, lines_containing};
use support::{eventually, meta, page_master, paged_dio, record};

const WITHIN: Duration = Duration::from_secs(5);

/// `rows` rows `r00..` with a `v` column, behind a master named `table`.
fn master(table: &str, rows: usize) -> Vista {
    let shell = (0..rows).fold(
        MockShell::new().with_metadata(meta(&["id", "v"])),
        |shell, i| {
            let id = format!("r{i:02}");
            shell.with_record(id.clone(), record(&[("id", &id), ("v", &id)]))
        },
    );
    Vista::new(table, shell)
}

fn entry(table: &str) -> Option<stats::FetchStats> {
    fetch_stats().into_iter().find(|s| s.table == table)
}

fn fetches(table: &str) -> usize {
    entry(table).map_or(0, |s| s.fetches)
}

/// A paged view over `dio` whose first page has landed.
async fn first_page(dio: &Dio) -> Arc<dyn TableScenery> {
    let view = dio.table_scenery().page_size(10).open().await.unwrap();
    view.set_viewport(0..10, 0..10);
    assert!(
        eventually(WITHIN, || view.row(9).is_some()).await,
        "the first page never landed"
    );
    view
}

/// Wait for the fetch count of `table` to reach past `before`.
async fn fetched_past(table: &str, before: usize) {
    assert!(
        eventually(WITHIN, || fetches(table) > before).await,
        "{table} was never fetched again"
    );
}

#[tokio::test]
async fn a_cold_page_is_counted_with_no_repeat_and_no_redundant_row() {
    reset_fetch_stats();
    let dio = paged_dio(master("ledger_cold", 30)).await;
    let _view = first_page(&dio).await;
    fetched_past("ledger_cold", 0).await;

    let stats = entry("ledger_cold").expect("the table was read");
    assert!(stats.fetches >= 1);
    assert_eq!(stats.repeats, 0, "{stats:?}");
    assert_eq!(stats.distinct_ranges, stats.fetches);
    assert_eq!(stats.worst_range, None, "nothing was asked for twice");
    assert_eq!(stats.rows_redundant, 0, "a cold cache held none of it");
    assert!(stats.rows_received >= 10, "{stats:?}");
    assert!(stats.ms_max <= stats.ms_total);
}

/// A forced refresh re-reads rows the cache already holds: those count as
/// redundant, legitimately.
#[tokio::test]
async fn a_forced_refresh_counts_the_rows_the_cache_already_held() {
    reset_fetch_stats();
    let dio = paged_dio(master("ledger_refresh", 30)).await;
    let view = first_page(&dio).await;
    fetched_past("ledger_refresh", 0).await;
    let before = fetches("ledger_refresh");

    view.refresh_source();
    fetched_past("ledger_refresh", before).await;

    let stats = entry("ledger_refresh").expect("the table was read");
    assert!(stats.rows_redundant > 0, "{stats:?}");
    assert_eq!(
        stats.repeats,
        stats.fetches - stats.distinct_ranges,
        "{stats:?}"
    );
}

#[tokio::test]
async fn a_range_asked_for_twice_is_a_repeat_and_the_worst_range() {
    reset_fetch_stats();
    let dio = paged_dio(master("ledger_repeat", 30)).await;
    let view = first_page(&dio).await;
    fetched_past("ledger_repeat", 0).await;
    let first = entry("ledger_repeat").expect("read");

    // The same block, forced from the master again until a range repeats.
    for _ in 0..5 {
        let before = fetches("ledger_repeat");
        view.refresh_source();
        fetched_past("ledger_repeat", before).await;
        if entry("ledger_repeat").is_some_and(|s| s.repeats > 0) {
            break;
        }
    }
    let stats = entry("ledger_repeat").expect("read");
    assert!(stats.fetches > first.fetches);
    assert!(stats.repeats >= 1, "{stats:?}");
    let (range, times) = stats.worst_range.clone().expect("a range repeated");
    assert!(times >= 2, "{stats:?}");
    assert!(range.contains(".."), "rendered as start..end: {range}");
}

#[tokio::test]
async fn reset_forgets_every_recorded_fetch() {
    let dio = paged_dio(master("ledger_reset", 30)).await;
    let _view = first_page(&dio).await;
    fetched_past("ledger_reset", 0).await;

    reset_fetch_stats();
    assert!(entry("ledger_reset").is_none(), "{:?}", fetch_stats());
}

#[tokio::test]
async fn the_busiest_table_comes_first() {
    reset_fetch_stats();
    let busy = paged_dio(master("ledger_busy", 30)).await;
    let busy_view = first_page(&busy).await;
    fetched_past("ledger_busy", 0).await;
    let quiet = paged_dio(master("ledger_quiet", 30)).await;
    let _quiet_view = first_page(&quiet).await;
    fetched_past("ledger_quiet", 0).await;
    while fetches("ledger_busy") <= fetches("ledger_quiet") {
        let before = fetches("ledger_busy");
        busy_view.refresh_source();
        fetched_past("ledger_busy", before).await;
    }

    let order: Vec<String> = fetch_stats().into_iter().map(|s| s.table).collect();
    let at = |table: &str| order.iter().position(|t| t == table).expect("listed");
    assert!(at("ledger_busy") < at("ledger_quiet"), "{order:?}");
}

#[tokio::test]
async fn the_summary_renders_a_header_each_table_the_live_counts_and_the_process() {
    reset_fetch_stats();
    let dio = paged_dio(master("ledger_summary", 30)).await;
    let _view = first_page(&dio).await;
    fetched_past("ledger_summary", 0).await;
    let stats = entry("ledger_summary").expect("read");

    let lines = debug_summary_lines();
    assert_eq!(lines[0], "— diorama session summary —");
    let table = lines
        .iter()
        .find(|l| l.starts_with("ledger_summary"))
        .expect("one line for the table");
    assert!(table.contains("summary"), "{table}");
    let counts = format!("{} fetches, {} repeats", stats.fetches, stats.repeats);
    assert!(table.contains(&counts), "{table}");
    assert!(
        table.contains(&format!("{} redundant", stats.rows_redundant)),
        "{table}"
    );
    assert!(
        table.contains("waiting") && table.contains("slowest"),
        "{table}"
    );
    let live = lines
        .iter()
        .find(|l| l.contains("still alive"))
        .expect("a live-counts line");
    assert!(live.contains("table scenery"), "{live}");
    let process = lines.last().expect("a process line");
    assert!(process.contains("session"), "{process}");
    assert!(process.contains("peak rss"), "{process}");
}

#[tokio::test]
async fn live_counts_follow_dios_views_and_servos_as_they_open_and_drop() {
    let before = live_counts();
    let lens = Lens::builder().cache(Cache::memory()).build().unwrap();
    let dio = lens.dio(master("census", 3)).build().await.unwrap();
    let table = dio.table_scenery().open().await.unwrap();
    let record = dio.record_scenery("r00").open().await.unwrap();
    let servo = dio.servo_new(IdStrategy::Uuid);

    let open = live_counts();
    assert!(open.dios > before.dios, "{open:?}");
    assert!(open.table_sceneries > before.table_sceneries, "{open:?}");
    assert!(open.record_sceneries > before.record_sceneries, "{open:?}");
    assert!(open.servos > before.servos, "{open:?}");

    drop((servo, record, table, dio, lens));
    let closed = async {
        loop {
            let now = live_counts();
            if now.dios <= before.dios
                && now.table_sceneries <= before.table_sceneries
                && now.record_sceneries <= before.record_sceneries
                && now.servos <= before.servos
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(WITHIN, closed)
        .await
        .unwrap_or_else(|_| panic!("still alive: {:?} (before {before:?})", live_counts()));
}

/// The only test here that turns a debug stream on: the summary stays silent
/// until some datasource has.
#[tokio::test]
async fn the_summary_is_emitted_only_once_a_datasource_enabled_the_stream() {
    let (_guard, log) = capture();
    assert!(!debug::any_tap_enabled());
    emit_debug_summary();
    assert!(log.lock().unwrap().is_empty(), "{:?}", log.lock().unwrap());

    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(page_master())
        .debug_datasource("ledger-ds")
        .build()
        .unwrap();
    assert!(debug::any_tap_enabled());
    let dio = lens.dio(master("ledger_emit", 30)).build().await.unwrap();
    let _view = first_page(&dio).await;

    emit_debug_summary();
    assert_eq!(lines_containing(&log, "session summary").len(), 1);
    assert!(!lines_containing(&log, "still alive").is_empty());
    assert!(!lines_containing(&log, "peak rss").is_empty());
}

#[test]
fn process_stats_report_a_peak_rss_and_never_run_backwards() {
    let first = debug::process_stats();
    let mut x = 0u64;
    for i in 0..2_000_000u64 {
        x = x.wrapping_add(i);
    }
    std::hint::black_box(x);
    let second = debug::process_stats();
    #[cfg(unix)]
    assert!(second.peak_rss_bytes > 0, "{second:?}");
    assert!(second.uptime_ms >= first.uptime_ms);
    assert!(second.cpu_ms >= first.cpu_ms);
}

#[test]
fn num_groups_thousands_with_commas() {
    assert_eq!(debug::num(0), "0");
    assert_eq!(debug::num(999), "999");
    assert_eq!(debug::num(1_000), "1,000");
    assert_eq!(debug::num(200_000), "200,000");
    assert_eq!(debug::num(1_234_567), "1,234,567");
}

#[test]
fn a_tap_is_off_by_default_and_names_no_datasource() {
    let off = debug::DebugTap::off();
    assert!(!off.enabled());
    assert_eq!(off.ds(), "");
    assert!(!debug::DebugTap::default().enabled());
}
