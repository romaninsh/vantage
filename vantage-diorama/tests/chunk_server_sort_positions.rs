//! A paged view whose master does the ordering must not keep the row
//! *positions* it had under the previous order.
//!
//! The cache holds whatever windows the view happened to visit — an arbitrary
//! subset of the old order, not a prefix of the new one. Re-sorting that subset
//! locally and seating it at rows 0..N claims those rows are the first N of the
//! sorted set, which they are not: under "by name descending" the true row 0 is
//! somewhere in the rows never fetched. The grid then reads as sorted at the top
//! and wrong everywhere else, mixed with correct rows wherever a later fetch
//! landed — which is exactly how it presented in the app.
//!
//! The fix drops positions (keeping the cached records) and lets the loader
//! refill the visible window from the master.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::SortDir;
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata};

mod support;
use support::chunk::{col_at, settle, Backend};

fn rec(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

/// A master that genuinely orders server-side, like SQL or a REST API with an
/// `order` parameter: `can_order` is true and the chunk callback serves the
/// window from the *sorted* set.
fn orderable_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("orderable"))
        .with_id_column("id");
    Vista::new(
        "items",
        Box::new(
            MockShell::new()
                .with_metadata(metadata)
                .with_capabilities(VistaCapabilities {
                    can_order: true,
                    ..Default::default()
                }),
        ),
    )
}

#[tokio::test]
async fn a_server_side_sort_does_not_leave_stale_row_positions() -> Result<()> {
    // 60 rows named a00..a59 — ascending by construction.
    let backend: Backend = Arc::new(Mutex::new(
        (0..60)
            .map(|i| (format!("id{i:02}"), rec(&format!("a{i:02}"))))
            .collect::<Vec<_>>(),
    ));

    let tmp = TempDir::new().expect("tempdir");
    let sorted = backend.clone();
    let counted = backend.clone();
    let lens = Arc::new(
        vantage_diorama::Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .total_provider(move |_dio| {
                let b = counted.clone();
                async move { Ok(b.lock().unwrap().len()) }
            })
            // Honours the sort the scenery asks for — a real orderable master.
            .on_load_chunk(move |_dio, range, query, sink| {
                let b = sorted.clone();
                async move {
                    let mut rows = b.lock().unwrap().clone();
                    if let Some((col, dir)) = &query.sort {
                        rows.sort_by(|a, z| {
                            let (x, y) = (a.1.get(col), z.1.get(col));
                            let ord = match (x, y) {
                                (Some(CborValue::Text(x)), Some(CborValue::Text(y))) => x.cmp(y),
                                _ => std::cmp::Ordering::Equal,
                            };
                            match dir {
                                vantage_diorama::SortDir::Desc => ord.reverse(),
                                _ => ord,
                            }
                        });
                    }
                    for idx in range {
                        if let Some((id, r)) = rows.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens.make_dio(orderable_master()).await?;
    let scenery = dio.table_scenery().page_size(20).open().await?;

    // Visit a window in the MIDDLE, so the cache holds rows that are not a
    // prefix of either order — the situation that made the bug visible.
    scenery.set_viewport(20..40);
    settle().await;
    settle().await;

    // Now sort descending at the source.
    scenery.set_sort(Some("name".to_string()), SortDir::Desc);
    settle().await;
    scenery.set_viewport(0..20);
    settle().await;
    settle().await;

    // Row 0 of "by name descending" over a00..a59 is a59. Before the fix this
    // was a20 — the first row of the stale cached window, locally re-sorted and
    // seated at position 0.
    assert_eq!(
        col_at(&scenery, 0, "name").as_deref(),
        Some("a59"),
        "row 0 must be the true first row of the new order, not a survivor of the old one",
    );
    assert_eq!(col_at(&scenery, 1, "name").as_deref(), Some("a58"));

    Ok(())
}
