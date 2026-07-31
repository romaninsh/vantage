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
use support::chunk::{Backend, col_at, settle};

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

/// The same shape, with a search narrowing the set first.
///
/// The guard above lets a view sort locally once it holds the whole set, and
/// "the whole set" is measured over the rows that match the current query —
/// not over everything the cache happens to hold. This case puts 40 cached
/// rows above a narrowed total of 20, with half the matching rows never
/// fetched, and asserts the top row is the true first of the narrowed order.
///
/// It passes against either measurement: a paged search refreshes the viewport
/// rather than reseeding from cache, so the guard is not what decides this
/// answer. Kept as coverage of paged search + server-side sort over a partly
/// cached set, which nothing else exercises.
#[tokio::test]
async fn a_narrowed_query_sorts_from_the_source_not_the_cached_sample() -> Result<()> {
    // a00..a09, then 30 rows that never match, then a10..a19. A view that
    // reads the first 40 rows caches ten matching rows and misses ten.
    let mut rows: Vec<(String, Record<CborValue>)> = Vec::new();
    for i in 0..10 {
        rows.push((format!("ida{i:02}"), rec(&format!("a{i:02}"))));
    }
    for i in 0..30 {
        rows.push((format!("idb{i:02}"), rec(&format!("b{i:02}"))));
    }
    for i in 10..20 {
        rows.push((format!("ida{i:02}"), rec(&format!("a{i:02}"))));
    }
    let backend: Backend = Arc::new(Mutex::new(rows));

    let tmp = TempDir::new().expect("tempdir");
    let served = backend.clone();
    let counted = backend.clone();
    let lens = Arc::new(
        vantage_diorama::Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .total_provider(move |_dio| {
                let b = counted.clone();
                async move { Ok(b.lock().unwrap().len()) }
            })
            // Orders AND searches at the source, and reports the narrowed
            // total with the window — a counted, searchable master.
            .on_load_chunk(move |_dio, range, query, sink| {
                let b = served.clone();
                async move {
                    let mut rows = b.lock().unwrap().clone();
                    if let Some(q) = &query.search {
                        rows.retain(|(_, r)| match r.get("name") {
                            Some(CborValue::Text(v)) => v.contains(q.as_str()),
                            _ => false,
                        });
                    }
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
                    sink.set_total(rows.len());
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

    // Fill the cache with 40 rows, only ten of which will match.
    scenery.set_viewport(0..40);
    settle().await;
    settle().await;

    // Narrow to the twenty "a" rows, then order them at the source.
    scenery.set_search(Some("a".to_string()));
    settle().await;
    scenery.set_sort(Some("name".to_string()), SortDir::Desc);
    settle().await;
    scenery.set_viewport(0..20);
    settle().await;
    settle().await;

    // Descending over a00..a19, row 0 is a19 — a row this cache never held.
    // Measuring coverage with the cache's 40 rows against the narrowed total
    // of 20 would seat a09 here instead.
    assert_eq!(
        col_at(&scenery, 0, "name").as_deref(),
        Some("a19"),
        "coverage must be counted over matching rows, not over everything cached",
    );

    Ok(())
}
