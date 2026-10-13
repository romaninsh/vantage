//! `TableScenery::count_where`: how many rows the view would hold with a
//! replacement set of UI filter terms.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{CappedScenery, Dio, LoadState, OpCondition, TableScenery};
use vantage_vista::{Capability, Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

mod support;
use support::{bucket_dio, eager_dio, open_listed, paged_dio, paged_or_eager_dio, record, text};

// 6 tasks: status open×4 / done×2; priority high×3 / low×3; titles "alpha…"×2.
const TASKS: [(&str, &str, &str, &str); 6] = [
    ("t1", "alpha one", "open", "high"),
    ("t2", "alpha two", "open", "high"),
    ("t3", "beta three", "open", "low"),
    ("t4", "beta four", "open", "low"),
    ("t5", "beta five", "done", "high"),
    ("t6", "beta six", "done", "low"),
];

fn cbor_array<const N: usize>(items: [&str; N]) -> CborValue {
    CborValue::Array(items.into_iter().map(text).collect())
}

/// A windowed, searchable master. Equality push-down only: it doesn't
/// advertise `FilterOps`, so it refuses every other operator.
fn tasks_master() -> Vista {
    let metadata = ["title", "status", "priority"]
        .into_iter()
        .fold(
            VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")),
            |m, c| m.with_column(Column::new(c, "String")),
        )
        .with_id_column("id");
    let caps = [Capability::Search, Capability::Window]
        .into_iter()
        .collect();
    let shell = TASKS.iter().fold(
        MockShell::new()
            .with_metadata(metadata)
            .with_capabilities(caps),
        |shell, (id, title, status, priority)| {
            shell.with_record(
                *id,
                record(&[
                    ("id", *id),
                    ("title", *title),
                    ("status", *status),
                    ("priority", *priority),
                ]),
            )
        },
    );
    Vista::new("tasks", shell)
}

/// The Dio is returned alongside the view: a scenery holds its Dio weakly, and
/// a closed Dio has nothing to count.
async fn paged_tasks() -> (Dio, Arc<dyn TableScenery>) {
    let dio = paged_dio(tasks_master()).await;
    let scenery = dio.table_scenery().open().await.expect("scenery opens");
    (dio, scenery)
}

async fn eager_tasks() -> (Dio, Arc<dyn TableScenery>) {
    let dio = eager_dio(tasks_master()).await;
    let scenery = dio.table_scenery().open().await.expect("scenery opens");
    (dio, scenery)
}

async fn wait_complete(scenery: &Arc<dyn TableScenery>) {
    let mut generations = scenery.subscribe();
    let complete = async {
        while scenery.load_state() != LoadState::Complete || scenery.row_count() < TASKS.len() {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    tokio::time::timeout(Duration::from_secs(3), complete)
        .await
        .expect("eager scenery never completed");
}

fn eq(column: &str, value: &str) -> OpCondition {
    OpCondition::new(column, FilterOp::Eq, text(value))
}

#[tokio::test]
async fn paged_counts_push_down_with_terms() {
    let (_dio, scenery) = paged_tasks().await;
    let open = eq("status", "open");
    assert_eq!(
        scenery.count_where(vec![open.clone()]).await.unwrap(),
        Some(4)
    );
    let both = vec![open, eq("priority", "high")];
    assert_eq!(scenery.count_where(both).await.unwrap(), Some(2));
    assert_eq!(scenery.count_where(vec![]).await.unwrap(), Some(6));
}

#[tokio::test]
async fn eager_counts_the_complete_cache() {
    let (_dio, scenery) = eager_tasks().await;
    wait_complete(&scenery).await;
    let terms = vec![OpCondition::new(
        "priority",
        FilterOp::InSet,
        cbor_array(["high"]),
    )];
    assert_eq!(scenery.count_where(terms).await.unwrap(), Some(3));
}

#[tokio::test]
async fn the_current_search_still_applies() {
    let (_dio, scenery) = paged_tasks().await;
    scenery.set_search(Some("alpha".into()));
    assert_eq!(scenery.count_where(vec![]).await.unwrap(), Some(2));
}

#[tokio::test]
async fn current_ui_terms_are_replaced_not_added() {
    let (_dio, scenery) = paged_tasks().await;
    scenery.set_filter_terms(vec![eq("status", "done")]);
    let open = vec![eq("status", "open")];
    assert_eq!(scenery.count_where(open).await.unwrap(), Some(4));
}

#[tokio::test]
async fn a_term_the_master_cannot_push_gives_no_count() {
    let (_dio, scenery) = paged_tasks().await;
    assert_eq!(scenery.count_where(not_done()).await.unwrap(), None);
}

/// A paged view whose Lens copies the whole set when a view leaves a clause
/// over counts that copy instead.
#[tokio::test]
async fn a_term_the_master_cannot_push_counts_the_whole_copy() {
    let dio = paged_or_eager_dio(tasks_master()).await;
    let scenery = dio.table_scenery().open().await.expect("scenery opens");
    assert_eq!(scenery.count_where(not_done()).await.unwrap(), Some(4));
}

#[tokio::test]
async fn a_cap_does_not_change_the_count() {
    let (_dio, scenery) = paged_tasks().await;
    let capped = CappedScenery::wrap(scenery, 2);
    assert_eq!(
        capped
            .count_where(vec![eq("status", "open")])
            .await
            .unwrap(),
        Some(4)
    );
}

#[tokio::test]
async fn a_two_pass_view_gives_no_count() {
    let dio = bucket_dio().await;
    let scenery = open_listed(&dio, 10).await;
    assert_eq!(scenery.count_where(vec![]).await.unwrap(), None);
}

#[tokio::test]
async fn a_closed_dio_gives_no_count() {
    let (dio, scenery) = paged_tasks().await;
    drop(dio);
    assert_eq!(scenery.count_where(vec![]).await.unwrap(), None);
}

fn not_done() -> Vec<OpCondition> {
    vec![OpCondition::new(
        "status",
        FilterOp::NotInSet,
        cbor_array(["done"]),
    )]
}

#[tokio::test]
async fn a_paged_view_supports_only_terms_its_master_can_push() {
    let (_dio, scenery) = paged_tasks().await;
    assert!(scenery.terms_supported(&[eq("status", "open")]));
    assert!(!scenery.terms_supported(&not_done()));
    let capped = CappedScenery::wrap(scenery, 2);
    assert!(!capped.terms_supported(&not_done()));
}

#[tokio::test]
async fn a_paged_view_with_a_whole_copy_supports_every_term() {
    let dio = paged_or_eager_dio(tasks_master()).await;
    let scenery = dio.table_scenery().open().await.expect("scenery opens");
    assert!(scenery.terms_supported(&not_done()));
}

#[tokio::test]
async fn an_eager_view_supports_every_term() {
    let (_dio, scenery) = eager_tasks().await;
    assert!(scenery.terms_supported(&not_done()));
    let capped = CappedScenery::wrap(scenery, 2);
    assert!(capped.terms_supported(&not_done()));
}

/// A two-pass view evaluates its terms over its hydrated rows, so a term on
/// an augmented column (`name`) is supported as well as one on a native
/// column (`kind`).
#[tokio::test]
async fn a_two_pass_view_supports_every_term() {
    let dio = bucket_dio().await;
    let scenery = open_listed(&dio, 10).await;
    let not_john = OpCondition::new("name", FilterOp::NotInSet, cbor_array(["John"]));
    let red = OpCondition::new("kind", FilterOp::Ne, text("red"));
    assert!(scenery.terms_supported(&[not_john, red]));
}

#[test]
fn the_future_runs_outside_tokio() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (_dio, scenery) = rt.block_on(paged_tasks());
    // futures' executor, not tokio's: count_where must not need a tokio context to be polled.
    let n = futures::executor::block_on(scenery.count_where(vec![])).unwrap();
    assert_eq!(n, Some(6));
}
