//! Shaping a view: order, builder and runtime filters, search, the one
//! query a paged loader receives, and a limit.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::Loader;
use vantage_vista::{Condition, FilterOp, SortDirection, VistaQuery};

use super::{build_lens, cbor_text, seeded_master, shown_ids, wait_for_gen};
use crate::support::{eventually, memory_lens};

#[tokio::test]
async fn set_order_reorders_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    scenery.set_order(Some(("name".into(), SortDirection::Descending)));
    wait_for_gen(&mut gen_rx, initial).await;

    let r0 = scenery.row(0).unwrap();
    let r2 = scenery.row(2).unwrap();
    assert_eq!(r0.record.get("name"), Some(&cbor_text("gamma")));
    assert_eq!(r2.record.get("name"), Some(&cbor_text("alpha")));
    Ok(())
}

#[tokio::test]
async fn a_builder_filter_narrows_the_view() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio
        .table_scenery()
        .filter(Condition::new("status", FilterOp::Eq, "open"))
        .open()
        .await?;
    let mut ids = shown_ids(&scenery);
    ids.sort();
    assert_eq!(ids, ["a", "c"]);
    Ok(())
}

#[tokio::test]
async fn set_filters_takes_any_operator() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;
    let scenery = dio.table_scenery().open().await?;
    let sorted_ids = || {
        let mut ids = shown_ids(&scenery);
        ids.sort();
        ids
    };

    scenery.set_filters(vec![Condition::new("n", FilterOp::Gt, 4)]);
    assert!(
        eventually(Duration::from_secs(5), || sorted_ids() == ["b", "c"]).await,
        "n > 4 never narrowed to b, c; shows {:?}",
        sorted_ids()
    );

    scenery.set_filters(vec![]);
    assert!(
        eventually(Duration::from_secs(5), || sorted_ids() == ["a", "b", "c"]).await,
        "clearing the filters never restored every row; shows {:?}",
        sorted_ids()
    );
    Ok(())
}

/// Every clause a view holds reaches the paged loader as one VistaQuery:
/// the builder filter, the runtime filters, the search and the order.
#[tokio::test]
async fn every_clause_reaches_the_paged_loader_in_one_query() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::<VistaQuery>::new()));
    let log = seen.clone();
    let loader = Loader::paged(move |cx, range, query, sink| {
        let log = log.clone();
        async move {
            log.lock().unwrap().push(query.clone());
            let fetched = cx.fetch(range.clone(), &query).await?;
            for (i, (id, row)) in fetched.page.rows.into_iter().enumerate() {
                sink.push(range.start + i, id, row).await?;
            }
            Ok(())
        }
    });
    let dio = memory_lens(loader).dio(seeded_master()).build().await?;
    let view = dio
        .table_scenery()
        .filter(Condition::new("status", FilterOp::Eq, "open"))
        .order("name", SortDirection::Ascending)
        .open()
        .await?;
    view.set_filters(vec![Condition::new("n", FilterOp::Gt, 1)]);
    view.set_search(Some("a".into()));
    view.set_viewport(0..10, 0..0);
    let wanted = |q: &VistaQuery| {
        q.conditions.len() == 2 && q.search.as_deref() == Some("a") && q.order.is_some()
    };
    assert!(
        eventually(Duration::from_secs(5), || seen
            .lock()
            .unwrap()
            .iter()
            .any(wanted))
        .await,
        "the loader never received the whole query; saw {:?}",
        seen.lock().unwrap()
    );
    Ok(())
}

/// A limit bounds every read to the cap, and the viewport with it.
#[tokio::test]
async fn a_limit_bounds_the_view_and_its_viewport() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let capped = dio.table_scenery().limit(2).open().await?;
    assert_eq!(capped.row_count(), 2);
    assert!(capped.row(1).is_some());
    assert!(capped.row(2).is_none()); // exists beneath, hidden by the cap
    assert!(!capped.has_more()); // the capped view is complete at the cap

    capped.set_viewport(0..10, 1..10);
    assert_eq!(capped.view_stats().showing, Some(1..2));

    // An order still applies, and the cap then shows the top of the new one.
    let mut gen_rx = capped.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    capped.set_order(Some(("price".to_string(), SortDirection::Ascending)));
    wait_for_gen(&mut gen_rx, initial).await;
    let r0 = capped.row(0).unwrap();
    assert_eq!(r0.record.get("name"), Some(&cbor_text("beta"))); // price 10

    // A cap wider than the data doesn't invent rows.
    let wide = dio.table_scenery().limit(10).open().await?;
    assert_eq!(wide.row_count(), 3);
    Ok(())
}
