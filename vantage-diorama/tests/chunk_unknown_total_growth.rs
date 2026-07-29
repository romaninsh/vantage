//! A window-paged source that never states a total.
//!
//! Offset-paginated APIs without a count endpoint exist everywhere: they
//! serve any `[offset, limit)` you ask for and tell you nothing about how
//! many rows there are. The only honest presentation is a list that grows as
//! you scroll — a full page means "there may be more", a short page means
//! "that was the end".
//!
//! Before the horizon rule, the first full page pinned the inferred total at
//! its own size: `row_count` equalled the loaded rows, the viewport could
//! never reach past them, and no scroll could ever request more — the set
//! froze at one page.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;

mod support;
use support::chunk::master as master_cols;

/// What the backend actually holds. Deliberately NOT a multiple of any page
/// size in play, so the exact end arrives as a short page.
const REAL_TOTAL: usize = 133;

fn rec(v: usize) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(format!("v{v}")));
    r
}

/// Paged lens with no `total_provider` and no `set_total` — the source
/// serves windows and states nothing, ever.
fn total_less_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    let rows: Vec<(String, Record<CborValue>)> = (0..REAL_TOTAL)
        .map(|i| (format!("id{i:04}"), rec(i)))
        .collect();
    Lens::new()
        .cache_at(cache)
        .on_load_chunk(move |_dio, range, _sort, sink| {
            let rows = rows.clone();
            async move {
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens")
}

async fn wait_for(what: &str, scenery: &Arc<dyn TableScenery>, mut ok: impl FnMut() -> bool) {
    let mut rx = scenery.subscribe();
    for _ in 0..100 {
        if ok() {
            return;
        }
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), rx.changed()).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn full_pages_extend_the_horizon_until_a_short_page_ends_it() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = total_less_lens(tmp.path().join("c.redb"));
    let dio = lens.make_dio(master_cols(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().open().await?;

    // The opening fetch returns a full page; the advertised set must reach
    // PAST the loaded rows, or no scroll can ever ask for more.
    wait_for("first page + horizon", &scenery, || {
        let n = scenery.row_count();
        n > 0 && scenery.row(n - 1).is_none()
    })
    .await;
    assert!(
        scenery.has_more(),
        "a total-less source with a full first page has more"
    );

    // Scroll to the horizon until the set stops growing; each round must
    // load rows the previous horizon exposed.
    let mut last = 0;
    for round in 0..64 {
        let n = scenery.row_count();
        if n == last && round > 0 {
            break;
        }
        last = n;
        scenery.set_viewport(n.saturating_sub(20)..n);
        wait_for("horizon rows load or end", &scenery, || {
            scenery.row_count() != n || scenery.row(n.saturating_sub(1)).is_some()
        })
        .await;
        // Let a follow-up horizon extension land before sampling.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        if scenery.row_count() == REAL_TOTAL {
            break;
        }
    }

    // The short page pinned the exact end.
    wait_for("exact total", &scenery, || {
        scenery.row_count() == REAL_TOTAL
    })
    .await;
    assert_eq!(scenery.estimated_total(), Some(REAL_TOTAL));

    // And every advertised row is real — no phantom tail left behind.
    wait_for("tail hydrated", &scenery, || {
        scenery.row(REAL_TOTAL - 1).is_some()
    })
    .await;

    Ok(())
}
