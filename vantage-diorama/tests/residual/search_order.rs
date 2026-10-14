//! A search or an order the master doesn't take: evaluated over an eager
//! view's complete cache, a loud failure over a paged view with no eager
//! start, and handed back by `Dio::fetch`. And an order the master takes,
//! which nothing re-sorts.

use std::sync::{Arc, Mutex};

use vantage_core::Result;
use vantage_diorama::{Clause, Loader};
use vantage_vista::{Capability, SortDirection, Vista, VistaQuery};

use super::fixtures::{assert_fails, ids, items, metadata, open, shell, wait_for_ids};
use crate::support::{eager_dio, memory_lens, paged_dio, search};

/// A master that can't search.
fn unsearchable() -> Vista {
    items(&[Capability::Order, Capability::FilterOps])
}

#[tokio::test]
async fn a_residual_search() -> Result<()> {
    let eager = eager_dio(unsearchable()).await;
    let view = open(&eager, |b| b).await;
    view.set_search(Some("re".into()));
    wait_for_ids(&view, &["a", "c"]).await;

    let paged = paged_dio(unsearchable()).await;
    let view = open(&paged, |b| b).await;
    view.set_search(Some("re".into()));
    assert_fails(&view, Clause::Search);

    let fetched = paged
        .fetch(0..10, &VistaQuery::new().with_search("re"))
        .await?;
    assert_eq!(fetched.refused.search.as_deref(), Some("re"));
    Ok(())
}

/// The Dio's own residual search is the only clause left over: an eager
/// view applies it, and a paged view, whose loader would read windows
/// outside the Dio, fails.
#[tokio::test]
async fn the_dio_search_as_the_only_residual() -> Result<()> {
    let eager = eager_dio(unsearchable())
        .await
        .with_query(&search("re"))
        .await?;
    assert_eq!(eager.residual().search.as_deref(), Some("re"));
    let view = open(&eager, |b| b).await;
    wait_for_ids(&view, &["a", "c"]).await;

    let paged = paged_dio(unsearchable())
        .await
        .with_query(&search("re"))
        .await?;
    let view = open(&paged, |b| b).await;
    assert_fails(&view, Clause::Search);

    let fetched = paged.fetch(0..10, &VistaQuery::new()).await?;
    assert_eq!(fetched.refused.search.as_deref(), Some("re"));
    Ok(())
}

/// The view's search and the Dio's are both left over: a row must match
/// both.
#[tokio::test]
async fn a_view_search_beside_the_dio_search() -> Result<()> {
    let eager = eager_dio(unsearchable())
        .await
        .with_query(&search("re"))
        .await?;
    let view = open(&eager, |b| b).await;
    view.set_search(Some("3".into()));
    wait_for_ids(&view, &["c"]).await;

    let paged = paged_dio(unsearchable())
        .await
        .with_query(&search("re"))
        .await?;
    let view = open(&paged, |b| b).await;
    view.set_search(Some("3".into()));
    assert_fails(&view, Clause::Search);
    Ok(())
}

/// An order the master refuses (`n` isn't `Orderable`).
#[tokio::test]
async fn a_residual_order() -> Result<()> {
    let all = &[Capability::Order, Capability::Search, Capability::FilterOps];
    let eager = eager_dio(items(all)).await;
    let view = open(&eager, |b| b.order("n", SortDirection::Descending)).await;
    wait_for_ids(&view, &["c", "b", "a"]).await;

    let paged = paged_dio(items(all)).await;
    let view = open(&paged, |b| b.order("n", SortDirection::Descending)).await;
    assert_fails(&view, Clause::Order("n".into()));

    let by_n = VistaQuery::new().with_order("n", SortDirection::Descending);
    let fetched = paged.fetch(0..10, &by_n).await?;
    assert_eq!(
        fetched.refused.order,
        Some(("n".to_string(), SortDirection::Descending))
    );
    Ok(())
}

/// An order the master takes is pushed, and the window is written as the
/// loader serves it: no client re-sort runs. The loader here hands its
/// window back reversed, so a local re-sort would show in the result.
#[tokio::test]
async fn a_pushed_order_is_never_re_sorted() -> Result<()> {
    let orders: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let seen = orders.clone();
    let loader = Loader::paged(move |cx, range, query, sink| {
        let seen = seen.clone();
        async move {
            seen.lock()
                .unwrap()
                .push(query.order.as_ref().map(|(column, _)| column.clone()));
            let fetched = cx.fetch(range.clone(), &query).await?;
            assert!(fetched.refused.is_empty(), "the master takes the order");
            sink.set_total(fetched.page.rows.len());
            for (i, (id, row)) in fetched.page.rows.into_iter().rev().enumerate() {
                sink.push(range.start + i, id, row).await?;
            }
            Ok(())
        }
    });
    let all = &[Capability::Order, Capability::Search, Capability::FilterOps];
    let master = Vista::new("items", shell(metadata(true), all));
    let dio = memory_lens(loader).dio(master).build().await?;

    let view = open(&dio, |b| b.order("n", SortDirection::Descending)).await;
    // The master orders c, b, a; the loader reverses it.
    wait_for_ids(&view, &["a", "b", "c"]).await;
    assert!(
        orders.lock().unwrap().contains(&Some("n".to_string())),
        "the order reached the loader"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(ids(&view), ["a", "b", "c"], "nothing re-sorted the window");
    Ok(())
}
