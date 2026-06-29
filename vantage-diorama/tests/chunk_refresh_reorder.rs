//! A chunk-loaded grid with NO client sort, showing the master's native order
//! (e.g. REST `?ordering=-last_updated`). When that native order SHIFTS between
//! refreshes (the simulator bumps `last_updated`) and the refresh re-fetches only
//! a partial viewport by absolute offset, a row that migrated into the refetched
//! window can be left ALSO occupying its old slot — a duplicate. This reproduces
//! that scramble.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Generation, Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn rec(name: &str, lu: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r.insert("lu".to_string(), CborValue::Integer(lu.into()));
    r
}

fn name_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery.row(idx).and_then(|r| match r.record.get("name") {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("lu", "i64"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

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
                // Native order = lu DESC (mirrors ?ordering=-last_updated).
                let mut rows = b.lock().unwrap().clone();
                rows.sort_by(|a, b| {
                    let la = a.1.get("lu");
                    let lb = b.1.get("lu");
                    match (la, lb) {
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
async fn refresh_after_reorder_does_not_duplicate_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Native order lu desc: a,b,c,d (a newest).
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("a", 40)),
        ("b".into(), rec("b", 30)),
        ("c".into(), rec("c", 20)),
        ("d".into(), rec("d", 10)),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    // Load the whole set, then narrow the live viewport to the top rows (the
    // grid's visible window is smaller than what it has loaded).
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..4);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    assert_eq!(name_at(&scenery, 0).as_deref(), Some("a"));
    assert_eq!(name_at(&scenery, 3).as_deref(), Some("d"));

    scenery.set_viewport(0..2); // visible window shrinks; last_viewport = 0..2
    settle().await;

    // Simulator bumps d to the top: native order becomes d,a,b,c.
    backend.lock().unwrap()[3].1 = rec("d", 99);
    let g = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;

    // Collect the visible names across the full row_count and assert no id/name
    // appears twice — a refetch of a shifted order must not duplicate rows.
    let names: Vec<String> = (0..scenery.row_count())
        .filter_map(|i| name_at(&scenery, i))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        before,
        "rows must not be duplicated after a reordering refresh; saw {names:?}"
    );
    Ok(())
}
