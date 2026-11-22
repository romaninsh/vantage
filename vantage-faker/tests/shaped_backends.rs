//! The `ShapedShell` contract: a shape's capabilities are enforced, its
//! pagination styles serve the same store, its faults fire on schedule, and
//! a seed replays the whole personality deterministically.
//!
//! The store underneath is a plain [`MemoryStore`] table, seeded through
//! [`DatasetGen`] — the same path an app uses — then wrapped in
//! [`ShapedShell`] to layer the personality on top.

use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::Inserted;
use vantage_dataset::prelude::{ReadableValueSet as _, WritableValueSet as _};
use vantage_faker::{
    BackendShape, DatasetGen, ExtraFields, FakerColumn, FaultSchedule, Latency, LatencyModel,
    Offline, ShapedShell, TableGen, WriteRefusal,
};
use vantage_memory::{MemoryStore, MemoryVistaFactory, MemoryVistaSpec};
use vantage_types::Record;
use vantage_vista::{
    Capability, ColumnFlag, ColumnSpec, FilterOp, Vista, VistaCapabilities, VistaFactory,
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

/// Mirror of the spec a real table config would declare: every generated
/// column is searchable (and, as every memory column, orderable), and the id
/// column carries the `id` flag.
fn spec(name: &str, columns: &[FakerColumn], id_column: &str) -> MemoryVistaSpec {
    let mut spec = MemoryVistaSpec::new(name);
    spec.id_column = Some(id_column.to_string());
    for c in columns {
        let mut col = ColumnSpec::new();
        col.col_type = Some(c.ty.clone());
        col.flags.push(ColumnFlag::Searchable);
        if c.name == id_column {
            col.flags.push(ColumnFlag::Id);
        }
        spec.columns.insert(c.name.clone(), col);
    }
    spec
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
    let spec = spec("shaped", &cols, "id");

    DatasetGen::new(seed)
        .table(table.columns(cols))
        .generate(&store)
        .expect("dataset generates");

    MemoryVistaFactory::new(store)
        .build_from_spec(spec)
        .unwrap()
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
    assert!(err.is_unavailable(), "an outage is transient: {err}");

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
    assert!(vista.fetch_window(0, 5).await.unwrap_err().is_unavailable());
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
    let vista = MemoryVistaFactory::new(store.clone())
        .from_yaml("name: t\ncolumns:\n  id: { type: string, flags: [id] }\n")
        .unwrap()
        .map_shell(|inner| ShapedShell::new(inner, BackendShape::default()));
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

/// Three rows behind a writable shape with these faults.
fn writable(faults: FaultSchedule, latency: LatencyModel) -> Vista {
    shaped(
        3,
        BackendShape {
            faults,
            latency,
            seed: Some(9),
            ..BackendShape::default()
        },
    )
}

fn ada() -> Record<CborValue> {
    [(
        "name".to_string(),
        CborValue::Text("written by the test".into()),
    )]
    .into_iter()
    .collect()
}

#[tokio::test(start_paused = true)]
async fn writes_pay_the_write_latency() {
    let vista = writable(
        FaultSchedule::default(),
        LatencyModel {
            write: Some(Latency::fixed(Duration::from_millis(700))),
            ..LatencyModel::default()
        },
    );

    let started = tokio::time::Instant::now();
    vista.insert_value("new", &ada()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(700));

    let started = tokio::time::Instant::now();
    vista.delete("new").await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(700));

    let started = tokio::time::Instant::now();
    vista.list_values().await.unwrap();
    assert!(
        started.elapsed().is_zero(),
        "reads do not pay the write band"
    );
}

#[tokio::test]
async fn write_error_rate_fails_writes_and_leaves_reads_alone() {
    let vista = writable(
        FaultSchedule {
            write_error_rate: 1.0,
            ..FaultSchedule::default()
        },
        LatencyModel::default(),
    );

    let err = vista.insert_value("new", &ada()).await.unwrap_err();
    assert!(err.is_unavailable(), "{err}");
    assert!(err.to_string().contains("write failed"), "{err}");
    let first = vista
        .list_values()
        .await
        .unwrap()
        .into_keys()
        .next()
        .unwrap();
    assert!(
        vista
            .patch_value(&first, &ada())
            .await
            .unwrap_err()
            .is_unavailable()
    );

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 3, "nothing was inserted");
    assert!(vista.get_value("new").await.unwrap().is_none());
    assert_ne!(
        rows[&first].get("name"),
        ada().get("name"),
        "nothing was patched"
    );
}

#[tokio::test]
async fn a_lost_reply_fails_the_write_after_it_lands() {
    let vista = writable(
        FaultSchedule {
            lost_reply_rate: 1.0,
            ..FaultSchedule::default()
        },
        LatencyModel::default(),
    );

    let err = vista.insert_value("new", &ada()).await.unwrap_err();
    assert!(err.is_unavailable(), "{err}");
    assert!(err.to_string().contains("lost the write's reply"), "{err}");
    let stored = vista
        .get_value("new")
        .await
        .unwrap()
        .expect("the row landed");
    assert_eq!(stored.get("name"), ada().get("name"));
}

#[tokio::test(start_paused = true)]
async fn a_queued_insert_is_accepted_now_and_lands_later() {
    let vista = writable(
        FaultSchedule {
            queue: Some(Duration::from_secs(2)),
            ..FaultSchedule::default()
        },
        LatencyModel::default(),
    );

    let inserted = vista.insert_value("new", &ada()).await.unwrap();
    assert!(matches!(inserted, Inserted::Accepted));
    assert!(vista.get_value("new").await.unwrap().is_none());

    tokio::time::sleep(Duration::from_millis(2001)).await;
    assert!(vista.get_value("new").await.unwrap().is_some());
}

#[tokio::test]
async fn a_refusing_source_refuses_every_write_by_kind() {
    for refusal in [
        WriteRefusal::Conflict,
        WriteRefusal::InvalidValue,
        WriteRefusal::Forbidden,
    ] {
        let vista = writable(
            FaultSchedule {
                refuse: Some(refusal),
                ..FaultSchedule::default()
            },
            LatencyModel::default(),
        );
        let err = vista.insert_value("new", &ada()).await.unwrap_err();
        let marked = match refusal {
            WriteRefusal::Conflict => err.is_conflict(),
            WriteRefusal::InvalidValue => err.is_invalid_value(),
            WriteRefusal::Forbidden => err.is_forbidden(),
        };
        assert!(marked, "{refusal:?}: {err}");
        let first = vista
            .list_values()
            .await
            .unwrap()
            .into_keys()
            .next()
            .unwrap();
        assert!(vista.delete(&first).await.is_err(), "{refusal:?}: delete");
        assert_eq!(vista.list_values().await.unwrap().len(), 3, "{refusal:?}");
    }
}

#[tokio::test]
async fn an_offline_source_refuses_writes_too() {
    let vista = writable(
        FaultSchedule {
            offline: Some(Offline {
                down: Duration::from_secs(60),
                period: Duration::from_secs(60),
            }),
            ..FaultSchedule::default()
        },
        LatencyModel::default(),
    );

    let err = vista.insert_value("new", &ada()).await.unwrap_err();
    assert!(
        err.is_unavailable() && err.to_string().contains("offline"),
        "{err}"
    );
}
