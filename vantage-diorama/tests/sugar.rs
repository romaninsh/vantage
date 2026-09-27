//! Sugar: display-only columns one scenery reads, never stored.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, SortDir, Sugar, SugarFn, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10))
        .with_record("c", record("gamma", 20));
    Vista::new("items", Box::new(shell))
}

async fn build_lens(cache_path: std::path::PathBuf) -> Result<Arc<Lens>> {
    let lens = Lens::new()
        .cache_at(cache_path)
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await
            }
        })
        .build()
        .expect("build lens");
    Ok(Arc::new(lens))
}

async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
    tokio::time::timeout(Duration::from_secs(10), async {
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

/// `twice` = price × 2.
fn doubling() -> Sugar {
    let apply: SugarFn = Arc::new(|rows| {
        Box::pin(async move {
            rows.into_iter()
                .map(|r| {
                    let price = r
                        .get("price")
                        .and_then(|v| v.as_integer())
                        .map(i128::from)
                        .unwrap_or(0);
                    let mut out = Record::new();
                    out.insert(
                        "twice".to_string(),
                        CborValue::Integer(((price * 2) as i64).into()),
                    );
                    Ok(out)
                })
                .collect()
        })
    });
    Sugar::new(apply, ["twice"])
}

fn names(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .map(|i| match scenery.row(i).unwrap().record.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            other => panic!("name: {other:?}"),
        })
        .collect()
}

fn twice_of(scenery: &Arc<dyn TableScenery>, name: &str) -> Option<CborValue> {
    (0..scenery.row_count())
        .map(|i| scenery.row(i).unwrap())
        .find(|r| r.record.get("name") == Some(&cbor_text(name)))
        .and_then(|r| r.record.get("twice").cloned())
}

#[tokio::test]
async fn a_sugared_view_sorts_by_an_output_and_a_plain_view_sees_none() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let sweet = dio
        .table_scenery()
        .sugar(doubling())
        .sort("twice", SortDir::Desc)
        .open()
        .await?;
    let plain = dio.table_scenery().open().await?;

    assert_eq!(names(&sweet), ["alpha", "gamma", "beta"]);
    assert_eq!(
        twice_of(&sweet, "beta"),
        Some(CborValue::Integer(20.into()))
    );
    assert_eq!(twice_of(&plain, "beta"), None);
    Ok(())
}

#[tokio::test]
async fn outputs_never_reach_the_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let sweet = dio.table_scenery().sugar(doubling()).open().await?;
    assert_eq!(sweet.row_count(), 3);
    let cached = dio.cache().get_value("a").await?.unwrap();
    assert!(cached.get("twice").is_none());
    Ok(())
}

#[tokio::test]
async fn a_changed_row_is_recomputed() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let sweet = dio.table_scenery().sugar(doubling()).open().await?;
    assert_eq!(
        twice_of(&sweet, "alpha"),
        Some(CborValue::Integer(60.into()))
    );
    let mut gen_rx = sweet.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.patched("a", record("alpha", 50)).await?;
    wait_for_gen(&mut gen_rx, initial).await;

    assert_eq!(
        twice_of(&sweet, "alpha"),
        Some(CborValue::Integer(100.into()))
    );
    Ok(())
}

#[tokio::test]
async fn a_record_scenery_reads_the_sugared_row() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let sweet = dio.record_scenery_sugared("b", doubling()).await?;
    let plain = dio.record_scenery("b").await?;
    let twice = |r: Option<Arc<vantage_diorama::EnrichedRecord>>| {
        r.and_then(|r| r.record.get("twice").cloned())
    };
    assert_eq!(twice(sweet.record()), Some(CborValue::Integer(20.into())));
    assert_eq!(twice(plain.record()), None);
    Ok(())
}
