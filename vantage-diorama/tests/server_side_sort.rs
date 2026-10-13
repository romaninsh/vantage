//! Server-side ordering: when the master advertises `Order` and the column is
//! `Orderable`, the scenery pushes its sort down (`Dio::fetch` → `with_query`
//! on a per-call clone → `fetch_window`) and does NOT re-sort client-side. An
//! order the master refuses fails a paged view with no eager start, and is
//! sorted over the whole copy when the Lens has one. Contrast `chunk_sort.rs`,
//! which covers a master without `Order`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Clause, LoadState, SortDir};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, SortDirection, Vista, VistaMetadata, VistaQuery,
    mocks::MockShell,
};

mod support;
use support::chunk::{col_at, wait_for_gen};
use support::{eager_dio, paged_dio};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

/// A master that can order + window server-side. Its *native* (insertion) order
/// (v3, v1, v2) is deliberately unsorted, so any sorted result must come from
/// the master applying `set_order` — there is no client-side sort in the helper.
fn orderable_master() -> Vista {
    // Orderable columns carry the ORDERABLE flag — exactly what the SQL / Mongo /
    // Surreal factories set so `Vista::set_order` accepts the column.
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String").with_flag(ColumnFlag::Orderable))
        .with_id_column("id");
    let caps = [Capability::Order, Capability::Window]
        .into_iter()
        .collect();
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(caps)
        .with_record("a", rec("v3"))
        .with_record("b", rec("v1"))
        .with_record("c", rec("v2"));
    Vista::new("items", shell)
}

fn vs(rows: &[(String, Record<CborValue>)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(_, r)| match r.get("v") {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn by_v(dir: SortDirection) -> VistaQuery {
    VistaQuery::new().with_order("v", dir)
}

#[tokio::test]
async fn fetch_orders_via_master() -> Result<()> {
    let dio = eager_dio(orderable_master()).await;

    // Ascending: the master returns v1, v2, v3 — its native order was v3,v1,v2,
    // so this ordering can only come from the pushed-down `set_order`.
    let asc = dio.fetch(0..10, &by_v(SortDirection::Ascending)).await?;
    assert!(asc.refused.is_empty(), "the master took the order");
    assert_eq!(
        vs(&asc.page.rows),
        vec!["v1", "v2", "v3"],
        "ascending, server-ordered"
    );

    // Descending, windowed: the top two by `v` desc.
    let desc_head = dio.fetch(0..2, &by_v(SortDirection::Descending)).await?;
    assert_eq!(
        vs(&desc_head.page.rows),
        vec!["v3", "v2"],
        "descending window"
    );

    // No sort → the master's NATIVE order, unpolluted. Checked *after* the two
    // ordered fetches above: because each fetch orders a per-call clone with
    // its own query state, the master itself is never reordered. (If the
    // clone shared the master's order — the bug this guards — this would come
    // back v3,v2,v1 from the last `set_order`.)
    let native = dio.fetch(0..10, &VistaQuery::new()).await?;
    assert_eq!(
        vs(&native.page.rows),
        vec!["v3", "v1", "v2"],
        "master native order intact — clones didn't mutate it"
    );

    Ok(())
}

/// An `Order`- and `Window`-capable master whose `v` isn't `Orderable`, so it
/// refuses a sort by `v`. Native order (v3, v1).
fn unorderable_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(
            [Capability::Order, Capability::Window]
                .into_iter()
                .collect(),
        )
        .with_record("a", rec("v3"))
        .with_record("b", rec("v1"));
    Vista::new("items", shell)
}

/// A sort an `Order`-capable master refuses (its column isn't `Orderable`)
/// comes back in `refused`, and the window in native order: the caller sees
/// that the page isn't sorted rather than being handed it as if it were.
#[tokio::test]
async fn a_refused_sort_on_an_ordering_master_is_returned() -> Result<()> {
    let dio = eager_dio(unorderable_master()).await;

    let fetched = dio.fetch(0..10, &by_v(SortDirection::Ascending)).await?;
    assert_eq!(
        fetched.refused.order,
        Some(("v".to_string(), SortDirection::Ascending)),
        "the refused order is handed back"
    );
    assert_eq!(vs(&fetched.page.rows), vec!["v3", "v1"], "native order");
    Ok(())
}

/// A paged view sorted by a column its `Order` master can't order, with no
/// eager start, fails loudly: no window is presented as the sorted set.
#[tokio::test]
async fn a_paged_view_sorted_by_a_refused_column_fails() -> Result<()> {
    let dio = paged_dio(unorderable_master()).await;
    let scenery = dio.table_scenery().open().await?;
    scenery.set_viewport(0..2);
    support::wait_listed(&scenery).await;

    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    match scenery.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(failure.table, "items");
            assert_eq!(failure.clause, Some(Clause::Order("v".into())));
        }
        other => panic!("expected a loud failure, got {other:?}"),
    }
    assert_eq!(scenery.row_count(), 0, "no rows for the refused sort");
    assert!(scenery.row(0).is_none());
    Ok(())
}

/// The same sort over a Lens with an eager start is served from the whole
/// copy, sorted locally.
#[tokio::test]
async fn an_eager_start_sorts_a_refused_order_locally() -> Result<()> {
    let dio = support::paged_or_eager_dio(unorderable_master()).await;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g0).await;
    support::wait_listed(&scenery).await;

    let got: Vec<Option<String>> = (0..2).map(|i| col_at(&scenery, i, "v")).collect();
    assert_eq!(got, vec![Some("v1".to_string()), Some("v3".to_string())]);
    assert_eq!(scenery.load_state(), LoadState::Complete);
    Ok(())
}

/// A paged scenery over an `Order` master orders **server-side**: the loader
/// receives the view's order in its query and reads the window through
/// `LoaderCx::fetch`, and — because the master took it — the scenery skips
/// the client re-sort, so the rows are written straight through in the
/// master's returned order.
#[tokio::test]
async fn paged_scenery_orders_server_side() -> Result<()> {
    let dio = paged_dio(orderable_master()).await;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..3);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(
        col_at(&scenery, 0, "v").as_deref(),
        Some("v3"),
        "native order"
    );

    let g1 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g1).await;
    let got: Vec<Option<String>> = (0..3).map(|i| col_at(&scenery, i, "v")).collect();
    assert_eq!(
        got,
        vec![
            Some("v1".to_string()),
            Some("v2".to_string()),
            Some("v3".to_string())
        ],
        "server-ordered ascending, no client re-sort"
    );
    Ok(())
}
