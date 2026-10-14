//! Sorting / filtering by a **dotted** column (`obj.field`) whose value lives in
//! a nested CBOR `Map` — the shape a REST `?mode=detailed` response produces for
//! belongs-to objects (e.g. `launch_service_provider.name`). The scenery must
//! resolve the path into the nested map, not look up the literal flat key.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Clause, LoadState, TableScenery};
use vantage_types::Record;
use vantage_vista::{
    Column, Condition, FilterOp, SortDirection, TableShell, Vista, VistaMetadata, VistaQuery,
    forward_table_shell, mocks::MockShell,
};

mod support;
use support::{eager_dio, paged_dio, paged_or_eager_dio, wait_listed};

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

/// A facade condition on a dotted path into a master column is evaluated over
/// the cache, where the nested value is resolved. A path whose root isn't a
/// master column is an error.
#[tokio::test]
async fn dotted_condition_is_evaluated_over_the_cache() -> Result<()> {
    let shell = master_shell();
    let dio = eager_dio(Vista::new("items", shell.clone())).await;
    // Upstream only: a cache-side evaluation never sees it.
    shell.set_record("d", record("Agency"));

    let agency = dio
        .cached()
        .with_condition("provider.name", FilterOp::Eq, "Agency")?;
    let ids: Vec<String> = agency.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec!["b"]);

    let err = dio
        .cached()
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

/// A fetch doesn't evaluate what the master leaves, so a dotted filter goes
/// to the master, which decides (a document store pushes it). This one
/// accepts it, so nothing comes back refused.
#[tokio::test]
async fn dotted_chunk_filter_reaches_the_master() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let master = master().map_shell(|inner| Recording {
        inner,
        seen: seen.clone(),
    });
    let dio = eager_dio(master).await;

    let query =
        VistaQuery::new().with_condition(Condition::new("provider.name", FilterOp::Ne, "Agency"));
    let fetched = dio.fetch(0..10, &query).await?;
    assert!(fetched.refused.is_empty(), "the master took the path");
    let columns: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.column.clone())
        .collect();
    assert_eq!(columns, ["provider.name"]);
    Ok(())
}

/// A paged view whose master refuses the dotted path, with no eager start,
/// fails loudly rather than show an unfiltered window as the filtered set.
/// With an eager start, the path is evaluated over the whole copy.
#[tokio::test]
async fn a_refused_dotted_chunk_filter_fails_a_paged_view() -> Result<()> {
    let filtered = |b: vantage_diorama::TableSceneryBuilder| {
        b.filter(Condition::new("provider.name", FilterOp::Ne, "Agency"))
    };

    let dio = paged_dio(master()).await;
    let scenery = filtered(dio.table_scenery()).open().await?;
    wait_listed(&scenery).await;
    match scenery.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(failure.clause, Some(Clause::Filter("provider.name".into())));
        }
        other => panic!("expected a loud failure, got {other:?}"),
    }
    assert_eq!(scenery.row_count(), 0);

    let dio = paged_or_eager_dio(master()).await;
    let scenery = filtered(dio.table_scenery()).open().await?;
    wait_listed(&scenery).await;
    let mut names: Vec<String> = (0..scenery.row_count())
        .filter_map(|i| provider_at(&scenery, i))
        .collect();
    names.sort();
    assert_eq!(names, ["Rocket Lab", "SpaceX"]);
    Ok(())
}

/// A facade over a master that declares no columns (only which one is the
/// id) can't check one, and takes any.
#[tokio::test]
async fn a_schemaless_facade_takes_any_column() -> Result<()> {
    let bare = MockShell::new().with_metadata(VistaMetadata::new().with_id_column("id"));
    let dio = eager_dio(Vista::new("bare", bare)).await;
    assert!(
        dio.cached()
            .with_condition("anything", FilterOp::Eq, "x")
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn sort_by_dotted_nested_column() -> Result<()> {
    let dio = eager_dio(master()).await;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let g = u64::from(*gen_rx.borrow_and_update());

    scenery.set_order(Some((
        "provider.name".to_string(),
        SortDirection::Ascending,
    )));
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
