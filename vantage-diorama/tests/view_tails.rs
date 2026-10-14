//! A tail over rows that arrive a page at a time: windows from a paged
//! source, with or without a total, and a two-pass list pass.

mod support;

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{Cache, Dio, Loader, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{SortDirection, TableShell, Vista, forward_table_shell};

use support::views::{ns, numbers_of, paged, serve_window};
use support::{augmented_dio, eventually, fixed_augmentation, quick_lens, size_detail};

/// Rows `r01`..`r{count}` with `n = 1..=count`.
fn counted(count: i64) -> MockShell {
    let ids: Vec<String> = (1..=count).map(|i| format!("r{i:02}")).collect();
    let rows: Vec<(&str, Option<i64>)> = ids
        .iter()
        .zip(1..)
        .map(|(id, n)| (id.as_str(), Some(n)))
        .collect();
    numbers_of(&rows)
}

/// Every load's window and order direction, served by [`serve_window`].
type Loads = Arc<Mutex<Vec<(Range<usize>, Option<SortDirection>)>>>;

fn recording_loader(loads: Loads, count: bool) -> Loader {
    Loader::paged(move |cx, range, query, sink| {
        let loads = loads.clone();
        async move {
            let dir = query.order.as_ref().map(|(_, dir)| *dir);
            loads.lock().unwrap().push((range.clone(), dir));
            serve_window(cx, range, query, sink, count).await
        }
    })
}

/// A tail of 2 by ascending `n` over 30 windowed rows, page size 10. The
/// Dio comes back with it: a scenery holds its Dio weakly.
async fn paged_tail(loads: &Loads, count: bool) -> Result<(Dio, Arc<dyn TableScenery>)> {
    let dio = paged(counted(30), recording_loader(loads.clone(), count)).await?;
    let view = dio
        .table_scenery()
        .page_size(10)
        .order("n", SortDirection::Ascending)
        .tail(2)
        .open()
        .await?;
    Ok((dio, view))
}

#[tokio::test]
async fn a_paged_tail_reads_the_last_rows() -> Result<()> {
    let loads = Loads::default();
    let (_dio, view) = paged_tail(&loads, true).await?;
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [29, 30]).await,
        "the tail shows {:?} after {:?}",
        ns(&view),
        loads.lock().unwrap()
    );
    let loads = loads.lock().unwrap().clone();
    assert!(
        loads.contains(&(28..30, Some(SortDirection::Ascending))),
        "the last window in the declared order: {loads:?}"
    );
    assert!(
        loads
            .iter()
            .all(|(_, dir)| *dir == Some(SortDirection::Ascending))
    );
    assert!(!view.has_more());
    Ok(())
}

/// Without a total the end is a horizon: the tail reads the flipped order.
#[tokio::test]
async fn a_paged_tail_without_a_total_reads_the_flipped_order() -> Result<()> {
    let loads = Loads::default();
    let (_dio, view) = paged_tail(&loads, false).await?;
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [29, 30]).await,
        "the tail shows {:?} after {:?}",
        ns(&view),
        loads.lock().unwrap()
    );
    let loads = loads.lock().unwrap().clone();
    assert!(
        loads
            .iter()
            .any(|(_, dir)| *dir == Some(SortDirection::Descending)),
        "the flipped order reached the loader: {loads:?}"
    );
    Ok(())
}

/// A detail source whose `get` for `r07` waits on `gate`. `done` counts the
/// lookups answered.
#[derive(Clone)]
struct Gated {
    inner: Box<dyn TableShell>,
    gate: Arc<Semaphore>,
    done: Arc<AtomicUsize>,
}

forward_table_shell!(Gated, inner, {
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        if id == "r07" {
            let _pass = self.gate.acquire().await;
        }
        let row = self.inner.get(id).await;
        self.done.fetch_add(1, Ordering::SeqCst);
        row
    }
});

/// A two-pass list pass lists a page at a time. An ordered view lists to the
/// end before it answers, and the tail counts back from there, not from the
/// end of the first page. Ordered by the augmented `size`, it also waits for
/// every row's detail: with `r07`'s held back, the refined map holds the six
/// others, and their last two are not the tail.
#[tokio::test]
async fn a_two_pass_tail_reads_the_last_rows_past_the_first_list_page() -> Result<()> {
    let ids: Vec<String> = (1..=7).map(|i| format!("r{i:02}")).collect();
    let sizes: Vec<String> = (1..=7).map(|i| i.to_string()).collect();
    let pairs = ids
        .iter()
        .zip(&sizes)
        .map(|(id, s)| (id.as_str(), s.as_str()));
    let (gate, done) = (Arc::new(Semaphore::new(0)), Arc::new(AtomicUsize::new(0)));
    let (held, answered) = (gate.clone(), done.clone());
    let sizes = Vista::new("sizes", size_detail(pairs)).map_shell(move |inner| Gated {
        inner,
        gate: held.clone(),
        done: answered.clone(),
    });
    let augmentation = fixed_augmentation(sizes, &["size"]);
    let lens = quick_lens(Cache::memory());
    let dio = augmented_dio(&lens, Vista::new("t", counted(7)), vec![augmentation]).await;
    // An unordered view lists one page: the cache holds the first 3 rows.
    let first_page = dio.table_scenery().page_size(3).open().await?;
    assert!(eventually(Duration::from_secs(5), || first_page.row_count() == 3).await);

    let view = dio
        .table_scenery()
        .page_size(3)
        .order("size", SortDirection::Ascending)
        .tail(2)
        .open()
        .await?;
    let only_the_tail = || {
        let shown = ns(&view);
        assert!(
            shown.is_empty() || shown == [6, 7],
            "the tail showed rows from before the end of the set: {shown:?}"
        );
        shown == [6, 7]
    };
    let six = eventually(Duration::from_secs(5), || done.load(Ordering::SeqCst) >= 6);
    assert!(six.await, "the other six rows never hydrated");
    // Long enough for the six to be published; the tail must not show them.
    assert!(!eventually(Duration::from_millis(300), only_the_tail).await);

    gate.add_permits(1);
    let answered = eventually(Duration::from_secs(5), only_the_tail);
    assert!(
        answered.await,
        "the tail never answered ({:?})",
        view.load_state()
    );
    assert_eq!(view.row_count(), 2);
    Ok(())
}
