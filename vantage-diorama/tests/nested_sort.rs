//! Sorting / filtering by a **dotted** column (`obj.field`) whose value lives in
//! a nested CBOR `Map` — the shape a REST `?mode=detailed` response produces for
//! belongs-to objects (e.g. `launch_service_provider.name`). The scenery must
//! resolve the path into the nested map, not look up the literal flat key.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChunkQuery, Lens, OpCondition, SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::{
    Column, Condition, FilterOp, TableShell, Vista, VistaMetadata, forward_table_shell,
    mocks::MockShell,
};

fn nested(provider_name: &str) -> CborValue {
    CborValue::Map(vec![(
        CborValue::Text("name".to_string()),
        CborValue::Text(provider_name.to_string()),
    )])
}

fn record(provider: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("provider".to_string(), nested(provider));
    r
}

fn provider_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    let row = scenery.row(idx)?;
    match row.record.get("provider")? {
        CborValue::Map(entries) => entries.iter().find_map(|(k, v)| match (k, v) {
            (CborValue::Text(k), CborValue::Text(s)) if k == "name" => Some(s.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn master_shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("provider", "String"))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(metadata)
        // Insertion order: SpaceX, Agency, Rocket Lab — NOT alphabetical.
        .with_record("a", record("SpaceX"))
        .with_record("b", record("Agency"))
        .with_record("c", record("Rocket Lab"))
}

fn master() -> Vista {
    Vista::new("items", master_shell())
}

async fn eager_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    )
}

/// A facade condition on a dotted path into a master column is evaluated over
/// the cache, where the nested value is resolved. A path whose root isn't a
/// master column is an error.
#[tokio::test]
async fn dotted_condition_is_evaluated_over_the_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("c.redb")).await;
    let shell = master_shell();
    let dio = lens.make_dio(Vista::new("items", shell.clone())).await?;
    // Upstream only: a cache-side evaluation never sees it.
    shell.set_record("d", record("Agency"));

    let agency = dio
        .vista()
        .with_condition("provider.name", FilterOp::Eq, "Agency")?;
    let ids: Vec<String> = agency.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec!["b"]);

    let err = dio
        .vista()
        .with_condition("client.name", FilterOp::Eq, "Agency")
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
    Ok(())
}

/// Records every condition the master receives. A dotted path stops here,
/// accepted, because the mock underneath doesn't evaluate nested paths; a
/// document store would push it.
#[derive(Clone)]
struct Recording {
    inner: Box<dyn TableShell>,
    seen: Arc<Mutex<Vec<Condition>>>,
}

forward_table_shell!(Recording, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.seen.lock().unwrap().push(condition.clone());
        if condition.column.contains('.') {
            return Ok(());
        }
        self.inner.add_condition(condition)
    }
});

/// A chunk fetch doesn't evaluate what the master leaves, so a dotted
/// filter goes to the master, which decides (a document store pushes it).
#[tokio::test]
async fn dotted_chunk_filter_reaches_the_master() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("c.redb")).await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let master = master().map_shell(|inner| Recording {
        inner,
        seen: seen.clone(),
    });
    let dio = lens.make_dio(master).await?;

    let query = ChunkQuery {
        filters: vec![OpCondition::new("provider.name", FilterOp::Ne, "Agency")],
        ..ChunkQuery::default()
    };
    dio.fetch_window_ordered(0, 10, query).await?;
    let columns: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.column.clone())
        .collect();
    assert_eq!(columns, ["provider.name"]);
    Ok(())
}

/// A facade over a master that declares no columns can't check one, and
/// takes any.
#[tokio::test]
async fn a_schemaless_facade_takes_any_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_lens(tmp.path().join("c.redb"))
        .await
        .make_dio(Vista::new("bare", MockShell::new()))
        .await?;
    assert!(
        dio.vista()
            .with_condition("anything", FilterOp::Eq, "x")
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn sort_by_dotted_nested_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("c.redb")).await;
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let g = u64::from(*gen_rx.borrow_and_update());

    scenery.set_sort(Some("provider.name".to_string()), SortDir::Asc);
    // Wait for the reseed bump. Bounds a hang, not a speed — see
    // `support/chunk.rs::wait_for_gen`.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if u64::from(*gen_rx.borrow_and_update()) > g {
                break;
            }
            gen_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("sort bump");

    // Alphabetical by provider.name: Agency, Rocket Lab, SpaceX.
    assert_eq!(provider_at(&scenery, 0).as_deref(), Some("Agency"));
    assert_eq!(provider_at(&scenery, 1).as_deref(), Some("Rocket Lab"));
    assert_eq!(provider_at(&scenery, 2).as_deref(), Some("SpaceX"));
    Ok(())
}
