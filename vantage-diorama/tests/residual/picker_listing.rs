//! A refined titles-only picker over more rows than one list page: it lists
//! its whole index before it shows an answer, and no update ever shows a row
//! its query or the Dio's residual excludes, nor a first-page answer as
//! complete.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use vantage_core::Result;
use vantage_diorama::{Cache, LoadState, TableScenery};
use vantage_vista::{Capability, FilterOp, Vista, VistaCapabilities, mocks::MockShell};

use super::fixtures::{ids, wait_for_ids};
use crate::support::{augment_name, augmented_dio, meta, quick_lens, record, wait_listed};

const ROWS: usize = 25;
const PAGE: usize = 10;

fn id(i: usize) -> String {
    format!("o{i:02}")
}

/// Row `i`'s kind: `violet` for row 22, past the first two list pages, and
/// otherwise red for odd rows and blue for even ones.
fn kind(i: usize) -> &'static str {
    match i {
        22 => "violet",
        i if i % 2 == 1 => "red",
        _ => "blue",
    }
}

/// [`ROWS`] bucket rows behind a master that can't take a non-`Eq`
/// operator, so a derived Dio's `kind != blue` stays in its residual.
fn long_bucket() -> Vista {
    let caps: VistaCapabilities = [Capability::Window, Capability::Search, Capability::Order]
        .into_iter()
        .collect();
    let shell = (1..=ROWS).fold(
        MockShell::new()
            .with_metadata(meta(&["id", "kind"]))
            .with_capabilities(caps),
        |shell, i| shell.with_record(id(i), record(&[("id", &id(i)), ("kind", kind(i))])),
    );
    Vista::new("bucket", shell)
}

/// The `(load state, ids)` a view showed at each generation.
type UpdateLog = Arc<Mutex<Vec<(LoadState, Vec<String>)>>>;

/// Every `(load state, ids)` the view shows, one per generation, from the
/// moment it is called until the returned handle is dropped.
fn record_updates(view: &Arc<dyn TableScenery>) -> (UpdateLog, tokio::task::JoinHandle<()>) {
    let seen = Arc::new(Mutex::new(vec![(view.load_state(), ids(view))]));
    let (log, watched) = (seen.clone(), view.clone());
    let mut generations = view.subscribe();
    let task = tokio::spawn(async move {
        while generations.changed().await.is_ok() {
            log.lock()
                .unwrap()
                .push((watched.load_state(), ids(&watched)));
        }
    });
    (seen, task)
}

/// No update showed an id outside `admitted`, and every one that read
/// `Complete` showed exactly `admitted`.
fn assert_only_admitted(seen: &[(LoadState, Vec<String>)], admitted: &[String]) {
    for (state, shown) in seen {
        assert!(
            shown.iter().all(|id| admitted.contains(id)),
            "an update showed an excluded row: {shown:?} ({state:?})",
        );
        if *state == LoadState::Complete {
            assert_eq!(shown, admitted, "a complete answer is the whole set");
        }
    }
}

async fn open_picker(dio: &vantage_diorama::Dio) -> Arc<dyn TableScenery> {
    dio.table_scenery()
        .titles_only()
        .page_size(PAGE)
        .open()
        .await
        .expect("picker opens")
}

#[tokio::test(flavor = "multi_thread")]
async fn no_update_of_a_narrowed_picker_shows_an_excluded_row() -> Result<()> {
    let parent = augmented_dio(
        &quick_lens(Cache::memory()),
        long_bucket(),
        vec![augment_name()],
    )
    .await;
    let narrowed = parent
        .with_query(&crate::support::cond("kind", FilterOp::Ne, "blue"))
        .await?;
    let admitted: Vec<String> = (1..=ROWS).filter(|i| kind(*i) != "blue").map(id).collect();

    let view = open_picker(&narrowed).await;
    let (seen, task) = record_updates(&view);
    wait_listed(&view).await;
    assert_eq!(ids(&view), admitted, "rows past the first page are listed");
    assert_eq!(view.load_state(), LoadState::Complete);

    view.request_load_more();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ids(&view), admitted, "load-more keeps the refined set");
    task.abort();
    assert_only_admitted(&seen.lock().unwrap(), &admitted);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_picker_search_finds_a_match_past_the_first_page() -> Result<()> {
    let dio = augmented_dio(
        &quick_lens(Cache::memory()),
        long_bucket(),
        vec![augment_name()],
    )
    .await;
    let view = open_picker(&dio).await;
    wait_listed(&view).await;
    assert_eq!(ids(&view).len(), PAGE, "unrefined, the picker pages");

    let (seen, task) = record_updates(&view);
    view.set_search(Some("violet".into()));
    wait_for_ids(&view, &["o22"]).await;
    wait_listed(&view).await;
    assert_eq!(view.load_state(), LoadState::Complete);
    task.abort();
    // The first page holds no match, so a search evaluated over it alone
    // shows a complete empty set.
    let seen = seen.lock().unwrap();
    assert!(
        !seen
            .iter()
            .any(|(state, shown)| *state == LoadState::Complete && shown.is_empty()),
        "first-page matches shown as complete: {seen:?}",
    );
    Ok(())
}
