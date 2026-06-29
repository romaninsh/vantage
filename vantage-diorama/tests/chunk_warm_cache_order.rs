//! A chunk-loaded grid's row ORDER is the master's (fetched per page), not the
//! cache's id-keyed iteration order. On reopen with a WARM cache (the redb file
//! survives a restart), the grid must still show the master's order — not seed
//! itself from the cache in id order and then skip the authoritative fetch.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::TableScenery;
use vantage_types::Record;
use vantage_vista::Vista;

mod support;
use support::chunk::{
    Backend, master as master_cols, order as order_col, paged_lens_native_desc, settle,
    wait_for_gen,
};

fn rec(name: &str, pos: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r.insert("pos".to_string(), CborValue::Integer(pos.into()));
    r
}

fn order(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    order_col(scenery, "name")
}

fn master() -> Vista {
    master_cols(&[("name", "String"), ("pos", "i64")])
}

/// `on_load_chunk` serves rows by `pos` DESC — the master's native order, which
/// is deliberately different from id order.
fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<vantage_diorama::Lens> {
    paged_lens_native_desc(cache, backend, "pos")
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
