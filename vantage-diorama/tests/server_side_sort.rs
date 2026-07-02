//! Server-side ordering: when the master `can_order`, the scenery pushes its
//! sort down (`Dio::fetch_window_ordered` → `add_order` on a per-call clone →
//! `fetch_window`) and does NOT re-sort client-side. Contrast `chunk_sort.rs`,
//! which covers the `can_order = false` fallback (client re-sort over the cache).

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Lens, SortDir};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

mod support;
use support::chunk::{col_at, wait_for_gen};
use support::eager_dio;

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

/// A master that can order + window server-side. Its *native* (insertion) order
/// (v3, v1, v2) is deliberately unsorted, so any sorted result must come from
/// the master applying `add_order` — there is no client-side sort in the helper.
fn orderable_master() -> Vista {
    // Orderable columns carry the ORDERABLE flag — exactly what the SQL / Mongo /
    // Surreal factories set so `Vista::add_order` accepts the column.
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String").with_flag(vantage_vista::flags::ORDERABLE))
        .with_id_column("id");
    let caps = VistaCapabilities {
        can_order: true,
        can_fetch_window: true,
        ..Default::default()
    };
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(caps)
        .with_record("a", rec("v3"))
        .with_record("b", rec("v1"))
        .with_record("c", rec("v2"));
    Vista::new("items", Box::new(shell))
}

fn vs(rows: &[(String, Record<CborValue>)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(_, r)| match r.get("v") {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn fetch_window_ordered_orders_via_master() -> Result<()> {
    let dio = eager_dio(orderable_master()).await;

    // Ascending: the master returns v1, v2, v3 — its native order was v3,v1,v2,
    // so this ordering can only come from the pushed-down `add_order`.
    let asc = dio
        .fetch_window_ordered(0, 10, Some(("v".to_string(), SortDir::Asc)))
        .await?;
    assert_eq!(
        vs(&asc),
        vec!["v1", "v2", "v3"],
        "ascending, server-ordered"
    );

    // Descending, windowed: the top two by `v` desc.
    let desc_head = dio
        .fetch_window_ordered(0, 2, Some(("v".to_string(), SortDir::Desc)))
        .await?;
    assert_eq!(vs(&desc_head), vec!["v3", "v2"], "descending window");

    // No sort → the master's NATIVE order, unpolluted. Checked *after* the two
    // ordered fetches above: because each fetch orders a per-call `clone_shell`
    // with its own query state, the master itself is never reordered. (If the
    // clone shared the master's order — the bug this guards — this would come
    // back v3,v2,v1 from the last `add_order`.)
    let native = dio.fetch_window_ordered(0, 10, None).await?;
    assert_eq!(
        vs(&native),
        vec!["v3", "v1", "v2"],
        "master native order intact — clones didn't mutate it"
    );

    Ok(())
}

/// A paged scenery over a `can_order` master orders **server-side**: the loader
/// hands the sort to `on_load_chunk` (which fetches via `fetch_window_ordered`),
/// and — because `can_order` — skips the client re-sort, so the rows are written
/// straight through in the master's returned order.
#[tokio::test]
async fn paged_scenery_orders_server_side() -> Result<()> {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .total_provider(|_dio| async { Ok(3) })
            .on_load_chunk(|dio, range, sort, sink| {
                let dio = dio.clone();
                async move {
                    let rows = dio
                        .fetch_window_ordered(range.start, range.end - range.start, sort)
                        .await?;
                    for (offset, (id, rec)) in rows.into_iter().enumerate() {
                        sink.push(range.start + offset, id, rec).await?;
                    }
                    Ok(())
                }
            })
            .build()
            .expect("lens builds"),
    );
    let dio = lens.make_dio(orderable_master()).await?;
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
