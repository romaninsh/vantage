//! A chunk-loaded grid's row ORDER is the master's (fetched per page), not the
//! cache's id-keyed iteration order. On reopen with a WARM cache (the redb file
//! survives a restart), the grid must still show the master's order — not seed
//! itself from the cache in id order and then skip the authoritative fetch.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Generation, Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn rec(name: &str, pos: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r.insert("pos".to_string(), CborValue::Integer(pos.into()));
    r
}

fn name_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery.row(idx).and_then(|r| match r.record.get("name") {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
}

fn order(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| name_at(scenery, i))
        .collect()
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("pos", "i64"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// `on_load_chunk` serves rows by `pos` DESC — the master's native order, which
/// is deliberately different from id order.
fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<Lens> {
    let total = backend.clone();
    let lens = Lens::new()
        .cache_at(cache)
        .total_provider(move |_dio| {
            let b = total.clone();
            async move { Ok(b.lock().unwrap().len()) }
        })
        .on_load_chunk(move |_dio, range, sink| {
            let b = backend.clone();
            async move {
                let mut rows = b.lock().unwrap().clone();
                rows.sort_by(|a, b| {
                    let pa = a.1.get("pos");
                    let pb = b.1.get("pos");
                    match (pa, pb) {
                        (Some(CborValue::Integer(x)), Some(CborValue::Integer(y))) => {
                            i128::from(*y).cmp(&i128::from(*x))
                        }
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .expect("build paged lens");
    Arc::new(lens)
}

async fn wait_for_gen(rx: &mut tokio::sync::watch::Receiver<Generation>, current: u64) -> u64 {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(80)).await;
}

#[tokio::test]
async fn warm_cache_reopen_shows_master_order_not_id_order() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("c.redb");
    // id order is 1,2,3; master (pos desc) order is 2,3,1.
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("1".into(), rec("one", 10)),
        ("2".into(), rec("two", 30)),
        ("3".into(), rec("three", 20)),
    ]));
    let lens = paged_lens(cache, backend.clone());
    let dio = lens.make_dio(master()).await?;

    // First open warms the cache via a viewport load.
    {
        let s1 = dio.table_scenery().open().await?;
        let mut rx = s1.subscribe();
        let g = u64::from(*rx.borrow_and_update());
        s1.set_viewport(0..3);
        wait_for_gen(&mut rx, g).await;
        settle().await;
        assert_eq!(
            order(&s1),
            vec!["two", "three", "one"],
            "master order on cold open"
        );
    }

    // Reopen with the warm cache — same Dio, cache retains the 3 rows.
    let s2 = dio.table_scenery().open().await?;
    let mut rx = s2.subscribe();
    s2.set_viewport(0..3);
    settle().await;
    let _ = wait_for_gen(&mut rx, 0).await;
    settle().await;
    assert_eq!(
        order(&s2),
        vec!["two", "three", "one"],
        "warm-cache reopen must show the master's order, not the cache's id order"
    );
    Ok(())
}
