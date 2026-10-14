//! A Dio derived with a condition or an order is what every scenery opened on
//! it sees: the Dio defines "what this table is", the view just observes it.

#[path = "../support/mod.rs"]
mod support;

mod derive;

use support::{MockView, eager_dio, teams_master};
use vantage_diorama::{Clause, Dio, LoadState};
use vantage_vista::{Condition, FilterOp, SortDirection};

#[tokio::test]
async fn dio_condition_is_inherited_by_every_view() {
    let dio: Dio = eager_dio(teams_master()).await;
    let red = dio
        .derive()
        .with_condition(Condition::new("team", FilterOp::Eq, "red"))
        .build()
        .await
        .expect("derives");

    let view = MockView::open(&red, 10).await;
    view.settle_until("filtered to red", |v| v.loaded_rows() == 2)
        .await;

    assert_eq!(view.loaded_rows(), 2, "only the two red rows show");
    assert_eq!(view.row_count(), 2);
    assert!(!view.is_loading());
}

#[tokio::test]
async fn no_condition_shows_all_rows() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.loaded_rows() == 3)
        .await;
    assert_eq!(view.loaded_rows(), 3);
}

#[tokio::test]
async fn dio_order_is_inherited_by_every_view() {
    let dio: Dio = eager_dio(teams_master()).await;
    let sorted = dio
        .derive()
        .with_order("team", SortDirection::Ascending)
        .build()
        .await
        .expect("derives");

    let view = MockView::open(&sorted, 10).await;
    view.settle_until("sorted", |v| v.loaded_rows() == 3).await;

    // Ascending by team: blue sorts before the two reds.
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
    assert_eq!(view.col_at(1, "team").as_deref(), Some("red"));
    assert_eq!(view.col_at(2, "team").as_deref(), Some("red"));
}

/// The teams under a master that can't search, so a search derived on it
/// stays in the Dio's residual.
fn unsearchable_teams() -> vantage_vista::Vista {
    use vantage_vista::{Capability, TableShell, Vista, mocks::MockShell};
    let shell = MockShell::new()
        .with_record("a", support::record(&[("id", "a"), ("team", "red")]))
        .with_record("b", support::record(&[("id", "b"), ("team", "blue")]))
        .with_record("c", support::record(&[("id", "c"), ("team", "red")]))
        .with_metadata(support::meta(&["id", "team"]));
    let caps = shell.capabilities().without(Capability::Search);
    Vista::new("members", shell.with_capabilities(caps))
}

/// Ids a view shows, sorted.
fn shown(view: &MockView) -> Vec<String> {
    let mut ids: Vec<String> = (0..view.row_count())
        .filter_map(|i| view.col_at(i, "id"))
        .collect();
    ids.sort();
    ids
}

/// A Dio's residual search is part of what the Dio is: a view's own search
/// narrows further, and clearing it never brings back a row outside the Dio.
#[tokio::test]
async fn a_view_search_never_replaces_the_dio_residual_search() {
    let dio = eager_dio(unsearchable_teams())
        .await
        .derive()
        .with_search("re")
        .build()
        .await
        .expect("derives");
    assert_eq!(dio.residual().search.as_deref(), Some("re"));

    let view = MockView::open(&dio, 10).await;
    view.settle_until("the two reds", |v| shown(v) == ["a", "c"])
        .await;

    // "b" matches the blue row's id, which the Dio's search excludes.
    view.scenery().set_search(Some("b".into()));
    for _ in 0..40 {
        assert!(!shown(&view).contains(&"b".to_string()), "b leaked in");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(shown(&view).is_empty(), "no red row holds a b");

    view.scenery().set_search(None);
    view.settle_until("back to the two reds", |v| shown(v) == ["a", "c"])
        .await;
    for _ in 0..40 {
        assert!(!shown(&view).contains(&"b".to_string()), "b leaked in");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// `Dio::fetch` on a derived Dio names the Dio's own residual in `refused`:
/// the master never applied it, so the page doesn't reflect it.
#[tokio::test]
async fn fetch_names_the_dio_residual_in_refused() {
    let dio = eager_dio(unsearchable_teams())
        .await
        .derive()
        .with_search("re")
        .build()
        .await
        .expect("derives");
    let fetched = dio
        .fetch(0..10, &vantage_vista::VistaQuery::new())
        .await
        .expect("fetches");
    assert_eq!(fetched.refused.search.as_deref(), Some("re"));
    assert_eq!(fetched.page.rows.len(), 3, "the master read is unnarrowed");
}

/// A paged view's search never replaces a search the Dio pushed to its
/// master. The master keeps the Dio's search, so the view's is left over, and
/// a paged view with no eager start fails on it rather than read a window
/// outside the Dio: the loader never sees the query.
#[tokio::test]
async fn a_view_search_never_replaces_a_pushed_dio_search() {
    use std::sync::{Arc, Mutex};
    use vantage_diorama::Loader;

    let refused: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let seen = refused.clone();
    let loader = Loader::paged(move |cx, range, query, sink| {
        let seen = seen.clone();
        async move {
            let fetched = cx.fetch(range.clone(), &query).await?;
            seen.lock().unwrap().push(fetched.refused.search.clone());
            for (i, (id, row)) in fetched.page.rows.into_iter().enumerate() {
                sink.push(range.start + i, id, row).await?;
            }
            Ok(())
        }
    });
    let dio = support::memory_lens(loader)
        .dio(teams_master())
        .build()
        .await
        .expect("dio builds")
        .derive()
        .with_search("re")
        .build()
        .await
        .expect("derives");
    assert_eq!(dio.master().query().search.as_deref(), Some("re"));

    let view = MockView::open(&dio, 10).await;
    view.settle_until("the two reds", |v| shown(v) == ["a", "c"])
        .await;

    view.scenery().set_search(Some("b".into()));
    match view.scenery().load_state() {
        LoadState::Failed(failure) => assert_eq!(failure.clause, Some(Clause::Search)),
        other => panic!("expected the view to fail on the search, got {other:?}"),
    }
    for _ in 0..40 {
        assert!(shown(&view).is_empty(), "a failed view shows no rows");
        assert!(
            !refused.lock().unwrap().contains(&Some("b".to_string())),
            "the loader never reads the query"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// A derived Dio keeps its own cache table, keyed by what narrows it, and
/// the master holds what it accepted while the residual holds the rest.
#[tokio::test]
async fn a_derived_dio_has_its_own_identity_and_residual() {
    let dio: Dio = eager_dio(teams_master()).await;
    let red = dio
        .derive()
        .with_condition(Condition::new("team", FilterOp::Eq, "red"))
        .build()
        .await
        .expect("derives");
    let sorted = red
        .derive()
        .with_order("team", SortDirection::Ascending)
        .build()
        .await
        .expect("derives");

    #[cfg(feature = "diagnostics")]
    {
        let table = async |d: &Dio| d.diagnostics().await.expect("diagnostics").cache_table;
        assert_eq!(table(&dio).await, "members");
        assert_eq!(
            table(&red).await,
            format!("members?{}", red.master().query().key())
        );
        assert_ne!(table(&red).await, table(&sorted).await);
    }
    assert!(
        red.residual().is_empty(),
        "the mock master takes an equality"
    );
    // The mock declares `team` without `Orderable`, so the order is refused
    // and kept as the residual, beside the master's applied condition.
    assert!(sorted.residual().order.is_some());
    assert_eq!(sorted.master().query().conditions.len(), 1);
}
