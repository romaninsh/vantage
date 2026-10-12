//! The `ShapedShell` contract: a shape's capabilities are enforced, its
//! pagination styles serve the same store, its faults fire on schedule, and
//! a seed replays the whole personality deterministically.
//!
//! The store underneath is a plain [`MemoryStore`] table, seeded through
//! [`DatasetGen`] — the same path an app uses — then wrapped in
//! [`ShapedShell`] to layer the personality on top.

use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet as _;
use vantage_faker::{
    BackendShape, DatasetGen, ExtraFields, FakerColumn, FaultSchedule, Latency, LatencyModel,
    Offline, ShapedShell, TableGen,
};
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableShell};
use vantage_vista::{
    Capability, Column, ColumnFlag, FilterOp, Vista, VistaCapabilities, VistaMetadata,
};

fn columns() -> Vec<FakerColumn> {
    vec![
        FakerColumn::new("id", "string"),
        FakerColumn::new("name", "string"),
        FakerColumn::new("surname", "string"),
        FakerColumn::new("age", "int"),
        FakerColumn::new("balance", "money"),
    ]
}

/// Mirror of the metadata a real table config would declare: every generated
/// column is orderable and searchable, and the id column carries the `id`
/// flag `MemoryTableShell` needs to answer traversal and title queries.
fn metadata(columns: &[FakerColumn], id_column: &str) -> VistaMetadata {
    let mut meta = VistaMetadata::new().with_id_column(id_column);
    for c in columns {
        let mut col = Column::new(&c.name, &c.ty)
            .with_flag(ColumnFlag::Orderable)
            .with_flag(ColumnFlag::Searchable);
        if c.name == id_column {
            col = col.with_flag(ColumnFlag::Id);
        }
        meta = meta.with_column(col);
    }
    meta
}

fn caps(list: &[Capability]) -> VistaCapabilities {
    list.iter().copied().collect()
}

fn windowed_caps() -> VistaCapabilities {
    caps(&[Capability::Window, Capability::Order])
}

fn shaped(count: usize, shape: BackendShape) -> Vista {
    shaped_rows(TableGen::new("shaped").count(count), shape)
}

/// A memory Vista over `table` (named `shaped`, given the fixture columns),
/// seeded from the shape's seed.
fn memory_rows(table: TableGen, seed: Option<u64>) -> Vista {
    let store = MemoryStore::new();
    let cols = columns();
    let meta = metadata(&cols, "id");

    DatasetGen::new(seed)
        .table(table.columns(cols))
        .generate(&store)
        .expect("dataset generates");

    let catalog = Catalog::new(store.clone());
    catalog.register("shaped", meta.clone());
    Vista::new(
        "shaped",
        MemoryTableShell::new(store.table("shaped"), meta, catalog),
    )
}

/// [`memory_rows`] wrapped in the shape.
fn shaped_rows(table: TableGen, shape: BackendShape) -> Vista {
    memory_rows(table, shape.seed).map_shell(|inner| ShapedShell::new(inner, shape))
}

fn ids(page: &vantage_vista::Page) -> Vec<&str> {
    page.rows.iter().map(|(id, _)| id.as_str()).collect()
}

#[tokio::test]
async fn advertised_window_serves_and_counts_with_total() {
    let vista = shaped(
        30,
        BackendShape {
            capabilities: windowed_caps(),
            seed: Some(1),
            ..BackendShape::default()
        },
    );

    let page = vista.fetch_window(5, 10).await.unwrap();
    assert_eq!(page.rows.len(), 10);
    assert_eq!(page.total, Some(30));

    // Windows tile without overlap when no skew is configured.
    let next = vista.fetch_window(15, 10).await.unwrap();
    assert!(ids(&page).iter().all(|id| !ids(&next).contains(id)));
}

#[tokio::test]
async fn unadvertised_operations_refuse_as_unsupported() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps(), // no cursor, no search, no writes
            seed: Some(1),
            ..BackendShape::default()
        },
    );

    let err = vista.fetch_cursor(None, 10).await.unwrap_err();
    assert!(err.is_unsupported(), "fetch_cursor must refuse: {err}");
    let err = vista.clone().with_search("x").unwrap_err();
    assert!(err.is_unsupported(), "search must refuse: {err}");

    let caps = vista.capabilities();
    assert!(!caps.has(Capability::Search) && !caps.has(Capability::Cursor));
    assert!(!caps.has(Capability::Insert) && !caps.has(Capability::Traverse));
}

#[tokio::test]
async fn a_shape_grants_nothing_the_inner_shell_lacks() {
    let vista = shaped(
        3,
        BackendShape {
            capabilities: Capability::iter().collect(),
            seed: Some(1),
            ..BackendShape::default()
        },
    );
    let caps = vista.capabilities();
    // Memory has no native conditions; import and traversal are never shaped.
    for refused in [
        Capability::NativeConditions,
        Capability::Import,
        Capability::Traverse,
    ] {
        assert!(!caps.has(refused), "{refused} must not be advertised");
    }
    assert!(caps.has(Capability::Cursor) && caps.has(Capability::ConfineWrites));
}

#[tokio::test]
async fn cursor_pagination_walks_the_whole_set_in_fixed_pages() {
    let vista = shaped(
        60,
        BackendShape {
            capabilities: caps(&[Capability::Cursor]),
            page_size: 25,
            seed: Some(2),
            ..BackendShape::default()
        },
    );

    let mut token = None;
    let mut seen = Vec::new();
    loop {
        // Asking for more than the server's page still gets one page.
        let (page, next) = vista.fetch_cursor(token, 100).await.unwrap();
        assert!(page.rows.len() <= 25);
        seen.extend(page.rows.into_iter().map(|(id, _)| id));
        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    assert_eq!(seen.len(), 60, "cursor walk covers every row exactly once");
    let mut dedup = seen.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), 60);
}

#[tokio::test]
async fn a_zero_cursor_limit_is_an_error() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: caps(&[Capability::Cursor]),
            seed: Some(2),
            ..BackendShape::default()
        },
    );
    let err = vista.fetch_cursor(None, 0).await.unwrap_err();
    assert!(
        err.to_string().contains("cursor limit must be positive"),
        "expected a limit error, got: {err}"
    );
}

#[tokio::test(start_paused = true)]
async fn expired_cursor_tokens_die() {
    let vista = shaped(
        60,
        BackendShape {
            capabilities: caps(&[Capability::Cursor]),
            page_size: 25,
            faults: FaultSchedule {
                cursor_expiry: Some(Duration::from_secs(30)),
                ..FaultSchedule::default()
            },
            seed: Some(2),
            ..BackendShape::default()
        },
    );

    let (_, token) = vista.fetch_cursor(None, 25).await.unwrap();
    let token = token.expect("more pages exist");

    tokio::time::advance(Duration::from_secs(31)).await;
    let err = vista.fetch_cursor(Some(token), 25).await.unwrap_err();
    assert!(
        err.to_string().contains("expired"),
        "expected token expiry, got: {err}"
    );
}

#[tokio::test(start_paused = true)]
async fn latency_is_paid_per_operation_class() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps(),
            latency: LatencyModel {
                window: Some(Latency::fixed(Duration::from_millis(800))),
                get: None, // instant get next to a slow window: the asymmetry
                ..LatencyModel::default()
            },
            seed: Some(3),
            ..BackendShape::default()
        },
    );

    let started = tokio::time::Instant::now();
    let fetched = vista.fetch_window(0, 5);
    tokio::pin!(fetched);
    // The paused clock only advances past the sleep when we let it.
    let page = fetched.await.unwrap();
    assert_eq!(page.rows.len(), 5);
    assert!(
        started.elapsed() >= Duration::from_millis(800),
        "window fetch must pay its latency toll"
    );
}

#[tokio::test(start_paused = true)]
async fn a_clone_keeps_its_search_toll() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps().with(Capability::Search),
            latency: LatencyModel {
                search_extra: Some(Latency::fixed(Duration::from_millis(500))),
                ..LatencyModel::default()
            },
            seed: Some(3),
            ..BackendShape::default()
        },
    );

    let searching = vista.with_search("a").unwrap();
    let clone = searching.clone();
    let started = tokio::time::Instant::now();
    clone.list_values().await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(500),
        "a clone of a searching handle is still searching"
    );

    let mut cleared = clone;
    cleared.set_search(None).unwrap();
    let started = tokio::time::Instant::now();
    cleared.list_values().await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[tokio::test(start_paused = true)]
async fn offline_windows_refuse_on_schedule() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps(),
            faults: FaultSchedule {
                offline: Some(Offline {
                    down: Duration::from_secs(10),
                    period: Duration::from_secs(60),
                }),
                ..FaultSchedule::default()
            },
            seed: Some(4),
            ..BackendShape::default()
        },
    );

    // t=0: online (windows start online).
    assert!(vista.fetch_window(0, 5).await.is_ok());

    // t=55s: inside the final 10s of the period — down.
    tokio::time::advance(Duration::from_secs(55)).await;
    let err = vista.fetch_window(0, 5).await.unwrap_err();
    assert!(err.to_string().contains("offline"), "got: {err}");

    // t=65s: next period, online again.
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(vista.fetch_window(0, 5).await.is_ok());
}

#[tokio::test]
async fn error_rate_one_fails_everything_and_totals_lie() {
    let vista = shaped(
        20,
        BackendShape {
            capabilities: windowed_caps(),
            faults: FaultSchedule {
                error_rate: 1.0,
                ..FaultSchedule::default()
            },
            seed: Some(5),
            ..BackendShape::default()
        },
    );
    assert!(vista.fetch_window(0, 5).await.is_err());
    assert!(vista.list_values().await.is_err());

    let honest_free = shaped(
        20,
        BackendShape {
            capabilities: windowed_caps(),
            faults: FaultSchedule {
                total_lie: -13,
                ..FaultSchedule::default()
            },
            seed: Some(5),
            ..BackendShape::default()
        },
    );
    assert_eq!(honest_free.count().await.unwrap(), 7);
    let page = honest_free.fetch_window(0, 5).await.unwrap();
    assert_eq!(page.total, Some(7), "the window's total lies the same way");
}

#[tokio::test]
async fn extra_fields_ride_along_undeclared() {
    let vista = shaped_rows(
        TableGen::new("shaped").count(3).extra_fields(ExtraFields {
            count: 50,
            size: 1000,
        }),
        BackendShape {
            seed: Some(6),
            ..BackendShape::default()
        },
    );
    let rows = vista.list_values().await.unwrap();
    let (_, rec) = rows.iter().next().unwrap();
    // 5 declared columns + 50 riders.
    assert_eq!(rec.len(), 55);
    let CborValue::Text(payload) = rec.get("extra_0050").unwrap() else {
        panic!("extra field should be text");
    };
    assert_eq!(payload.len(), 1000);
}

#[tokio::test]
async fn a_seed_replays_the_same_backend() {
    let shaped = || {
        shaped_rows(
            TableGen::new("shaped").count(25).weirdness(0.2),
            BackendShape {
                capabilities: windowed_caps(),
                seed: Some(42),
                ..BackendShape::default()
            },
        )
    };
    let a = shaped().list_values().await.unwrap();
    let b = shaped().list_values().await.unwrap();
    assert_eq!(
        a, b,
        "same seed, same rows — a scenario replays identically"
    );
}

#[tokio::test]
async fn search_gates_and_narrows_when_advertised() {
    let vista = shaped(
        40,
        BackendShape {
            capabilities: windowed_caps().with(Capability::Search),
            seed: Some(7),
            ..BackendShape::default()
        },
    );

    let all = vista.list_values().await.unwrap();
    // Pick a needle from a real row so the search must hit at least once.
    let needle = all
        .values()
        .find_map(|r| match r.get("name") {
            Some(CborValue::Text(s)) if s.len() >= 3 => Some(s[..3].to_string()),
            _ => None,
        })
        .expect("a generated name to search for");

    let mut vista = vista;
    vista.set_search(Some(&needle)).unwrap();
    let narrowed = vista.list_values().await.unwrap();
    assert!(!narrowed.is_empty());
    assert!(narrowed.len() <= all.len());
}

#[tokio::test]
async fn shaping_a_narrowed_vista_keeps_its_name_and_query() {
    let memory = memory_rows(TableGen::new("shaped").count(20), Some(8));
    let first = memory
        .list_values()
        .await
        .unwrap()
        .into_keys()
        .next()
        .unwrap();
    let narrowed = memory
        .with_condition("id", FilterOp::Eq, CborValue::Text(first.clone()))
        .unwrap();
    let shaped = narrowed.map_shell(|inner| ShapedShell::new(inner, BackendShape::default()));

    assert_eq!(shaped.name(), "shaped");
    assert!(shaped.is_narrowed());
    assert_eq!(shaped.query().conditions.len(), 1);
    let rows = shaped.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), [&first]);
}

#[tokio::test]
async fn a_shaped_memory_table_pushes_changes() {
    use futures_util::StreamExt;
    let store = MemoryStore::new();
    let t = store.table("t");
    let shell = MemoryTableShell::new(
        t.clone(),
        VistaMetadata::new()
            .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
            .with_id_column("id"),
        Catalog::new(store.clone()),
    );
    let vista =
        Vista::new("t", shell).map_shell(|inner| ShapedShell::new(inner, BackendShape::default()));
    assert!(vista.capabilities().has(Capability::Subscribe));
    let mut w = vista.watch().await.unwrap();
    t.upsert("a", Default::default());
    let change = tokio::time::timeout(std::time::Duration::from_secs(2), w.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        change,
        vantage_vista::VistaChange::Inserted { .. }
    ));
}
