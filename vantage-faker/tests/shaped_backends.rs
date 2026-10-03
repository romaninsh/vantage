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
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, flags};

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
            .with_flag(flags::ORDERABLE)
            .with_flag(flags::SEARCHABLE);
        if c.name == id_column {
            col = col.with_flag(flags::ID);
        }
        meta.columns.insert(c.name.clone(), col);
    }
    meta
}

fn windowed_caps() -> VistaCapabilities {
    VistaCapabilities {
        can_count: true,
        can_fetch_window: true,
        can_order: true,
        ..VistaCapabilities::default()
    }
}

fn shaped(count: usize, shape: BackendShape) -> Vista {
    shaped_rows(TableGen::new("shaped").count(count), shape)
}

/// Seed `table` (named `shaped`, given the fixture columns) from the
/// shape's seed, then wrap it in the shape.
fn shaped_rows(table: TableGen, shape: BackendShape) -> Vista {
    let store = MemoryStore::new();
    let cols = columns();
    let meta = metadata(&cols, "id");

    DatasetGen::new(shape.seed)
        .table(table.columns(cols))
        .generate(&store)
        .expect("dataset generates");

    let catalog = Catalog::new(store.clone());
    catalog.register("shaped", meta.clone());
    let table = MemoryTableShell::new(store.table("shaped"), meta, catalog);
    Vista::new("shaped", Box::new(ShapedShell::new(Box::new(table), shape)))
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

    let (rows, total) = vista.fetch_window_counted(5, 10).await.unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(total, Some(30));

    // Windows tile without overlap when no skew is configured.
    let (next, _) = vista.fetch_window_counted(15, 10).await.unwrap();
    assert!(rows.iter().all(|(id, _)| next.iter().all(|(n, _)| n != id)));
}

#[tokio::test]
async fn unadvertised_operations_refuse_as_unsupported() {
    let vista = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps(), // no fetch_page, no fetch_next, no search
            seed: Some(1),
            ..BackendShape::default()
        },
    );

    let page = vista.fetch_page(1).await;
    assert!(page.is_err(), "fetch_page must refuse when not advertised");
    let next = vista.fetch_next(None).await;
    assert!(next.is_err(), "fetch_next must refuse when not advertised");

    let caps = vista.capabilities();
    assert!(!caps.can_search && !caps.can_fetch_page && !caps.can_fetch_next);
}

#[tokio::test]
async fn cursor_pagination_walks_the_whole_set_in_fixed_pages() {
    let vista = shaped(
        60,
        BackendShape {
            capabilities: VistaCapabilities {
                can_fetch_next: true,
                ..VistaCapabilities::default()
            },
            page_size: 25,
            seed: Some(2),
            ..BackendShape::default()
        },
    );

    let mut token = None;
    let mut seen = Vec::new();
    loop {
        let (rows, next) = vista.fetch_next(token).await.unwrap();
        seen.extend(rows.into_iter().map(|(id, _)| id));
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

#[tokio::test(start_paused = true)]
async fn expired_cursor_tokens_die() {
    let vista = shaped(
        60,
        BackendShape {
            capabilities: VistaCapabilities {
                can_fetch_next: true,
                ..VistaCapabilities::default()
            },
            page_size: 25,
            faults: FaultSchedule {
                cursor_expiry: Some(Duration::from_secs(30)),
                ..FaultSchedule::default()
            },
            seed: Some(2),
            ..BackendShape::default()
        },
    );

    let (_, token) = vista.fetch_next(None).await.unwrap();
    let token = token.expect("more pages exist");

    tokio::time::advance(Duration::from_secs(31)).await;
    let err = vista.fetch_next(Some(token)).await.unwrap_err();
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
    let rows = fetched.await.unwrap();
    assert_eq!(rows.len(), 5);
    assert!(
        started.elapsed() >= Duration::from_millis(800),
        "window fetch must pay its latency toll"
    );
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
    assert_eq!(honest_free.get_count().await.unwrap(), 7);
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
    let mut caps = windowed_caps();
    caps.can_search = true;
    let vista = shaped(
        40,
        BackendShape {
            capabilities: caps,
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
    vista.add_search(&needle).unwrap();
    let narrowed = vista.list_values().await.unwrap();
    assert!(!narrowed.is_empty());
    assert!(narrowed.len() <= all.len());
}

#[tokio::test]
async fn a_shaped_memory_table_pushes_changes() {
    use futures_util::StreamExt;
    let store = MemoryStore::new();
    let t = store.table("t");
    let shell = MemoryTableShell::new(
        t.clone(),
        VistaMetadata::new()
            .with_column(Column::new("id", "string").with_flag("id"))
            .with_id_column("id"),
        Catalog::new(store.clone()),
    );
    let shaped = ShapedShell::new(Box::new(shell), BackendShape::default());
    let vista = Vista::new("t", Box::new(shaped));
    assert!(vista.can_watch());
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
