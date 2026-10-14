//! A view capped by the builder: `limit(n)` shows the first `n` rows of its
//! order, `tail(n)` the last `n`, and `row_by_id` finds a row only inside
//! the cap.

mod support;

use std::sync::Arc;
use std::time::Duration;

use vantage_core::Result;
use vantage_diorama::{ChangeEvent, Dio, LoadState, TableScenery};
use vantage_vista::mocks::MockShell;
use vantage_vista::{SortDirection, Vista};

use support::views::{ids, numbered, numbers_of, ns, shell};
use support::{eager_dio, eventually};

/// Rows `a`..`e` with `n = 1..=5`.
fn numbers() -> MockShell {
    numbers_of(&[("a", Some(1)), ("b", Some(2)), ("c", Some(3)), ("d", Some(4)), ("e", Some(5))])
}

/// An eager tail of `n` by ascending `n`. The Dio comes back with it: a
/// scenery holds its Dio weakly.
async fn ascending_tail(shell: MockShell, n: usize) -> Result<(Dio, Arc<dyn TableScenery>)> {
    let dio = eager_dio(Vista::new("t", shell)).await;
    let view = dio
        .table_scenery()
        .order("n", SortDirection::Ascending)
        .tail(n)
        .open()
        .await?;
    Ok((dio, view))
}

#[tokio::test]
async fn a_limit_caps_the_view() -> Result<()> {
    let dio = eager_dio(Vista::new("t", numbers())).await;
    let view = dio.table_scenery().limit(2).open().await?;
    assert!(eventually(Duration::from_secs(5), || view.row_count() == 2).await);
    assert_eq!(view.estimated_total(), Some(2));
    assert!(!view.has_more());
    assert!(view.row(2).is_none(), "the third row is beyond the cap");
    assert_eq!(view.view_stats().total, Some(2));
    Ok(())
}

#[tokio::test]
async fn a_tail_shows_the_last_rows_in_order() -> Result<()> {
    let (_dio, view) = ascending_tail(numbers(), 2).await?;
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [4, 5]).await,
        "the tail shows {:?}",
        ns(&view)
    );
    assert_eq!(
        view.view_stats().order,
        Some(("n".into(), SortDirection::Ascending))
    );
    Ok(())
}

/// Nulls sort last and ties break on the id in both directions, so the
/// flipped order read backwards is not the declared order's end.
#[tokio::test]
async fn a_tail_places_nulls_and_ties_as_the_declared_order_does() -> Result<()> {
    let rows = [("a", Some(1)), ("b", Some(2)), ("c", Some(3)), ("d", None), ("e", Some(3))];
    let (_dio, view) = ascending_tail(numbers_of(&rows), 3).await?;
    // Ascending: a 1, b 2, c 3, e 3 (the tie on id), d null last.
    assert!(
        eventually(Duration::from_secs(5), || ids(&view) == ["c", "e", "d"]).await,
        "the tail shows {:?}",
        ids(&view)
    );
    Ok(())
}

#[tokio::test]
async fn a_tail_view_follows_an_insert_at_the_end() -> Result<()> {
    let (dio, view) = ascending_tail(numbers(), 2).await?;
    assert!(eventually(Duration::from_secs(5), || ns(&view) == [4, 5]).await);

    dio.apply_change(ChangeEvent::Inserted {
        id: "f".into(),
        new: Some(numbered("f", Some(6))),
    })
    .await?;
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [5, 6]).await,
        "the tail never followed the insert; shows {:?}",
        ns(&view)
    );
    Ok(())
}

#[tokio::test]
async fn a_tail_without_an_order_is_refused_at_open() -> Result<()> {
    let dio = eager_dio(Vista::new("t", numbers())).await;
    let unordered = dio.table_scenery().tail(2).open().await;
    let message = unordered.err().expect("an unordered tail opens").to_string();
    assert!(message.contains("tail needs an order"), "{message}");

    let both = dio
        .table_scenery()
        .order("n", SortDirection::Ascending)
        .limit(2)
        .tail(2)
        .open()
        .await;
    let message = both.err().expect("limit with tail opens").to_string();
    assert!(message.contains("limit and tail are exclusive"), "{message}");
    Ok(())
}

#[tokio::test]
async fn clearing_the_order_of_a_tail_view_fails_it() -> Result<()> {
    let (_dio, view) = ascending_tail(numbers(), 2).await?;
    assert!(eventually(Duration::from_secs(5), || ns(&view) == [4, 5]).await);

    view.set_order(None);
    let failed = view.load_state();
    assert!(matches!(failed, LoadState::Failed(_)), "{failed:?}");
    assert_eq!(view.row_count(), 0);
    assert_eq!(view.estimated_total(), Some(0));
    assert_eq!(view.view_stats().total, view.estimated_total());

    view.set_search(Some("x".into()));
    view.set_search(None);
    view.set_order(Some(("n".into(), SortDirection::Ascending)));
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [4, 5]).await,
        "the order came back but the tail shows {:?}",
        ns(&view)
    );
    Ok(())
}

#[tokio::test]
async fn clearing_a_tail_order_keeps_the_dio_order() -> Result<()> {
    let dio = eager_dio(Vista::new("t", numbers())).await;
    let descending = dio
        .derive()
        .with_order("n", SortDirection::Descending)
        .build()
        .await?;
    let view = descending
        .table_scenery()
        .order("n", SortDirection::Ascending)
        .tail(2)
        .open()
        .await?;
    assert!(eventually(Duration::from_secs(5), || ns(&view) == [4, 5]).await);

    view.set_order(None);
    assert!(
        eventually(Duration::from_secs(5), || ns(&view) == [2, 1]).await,
        "the Dio's descending order should stand in; shows {:?} ({:?})",
        ns(&view),
        view.load_state()
    );
    Ok(())
}

#[tokio::test]
async fn row_by_id_reads_the_row_under_any_order() -> Result<()> {
    let dio = eager_dio(Vista::new("t", shell())).await;
    let view = dio
        .table_scenery()
        .order("name", SortDirection::Descending)
        .open()
        .await?;
    assert!(eventually(Duration::from_secs(5), || ids(&view) == ["c", "b", "a"]).await);
    let alpha = view.row_by_id("a").expect("alpha is in the view");
    assert_eq!(
        alpha.record.get("name").and_then(|v| v.as_text()),
        Some("alpha")
    );
    assert!(view.row_by_id("zz").is_none());

    let (_numbers, tail) = ascending_tail(numbers(), 2).await?;
    assert!(eventually(Duration::from_secs(5), || ns(&tail) == [4, 5]).await);
    assert!(tail.row_by_id("e").is_some(), "the last row is in the tail");
    assert!(tail.row_by_id("d").is_some());
    assert!(tail.row_by_id("a").is_none(), "the first row is not");
    Ok(())
}
