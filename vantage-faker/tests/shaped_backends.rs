//! The `ShapedShell` contract: a shape's capabilities are enforced, its
//! pagination styles serve the same store, its faults fire on schedule, and
//! a seed replays the whole personality deterministically.

use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet as _;
use vantage_faker::{
    BackendShape, ExtraFields, FakerColumn, FakerTable, FaultSchedule, Latency, LatencyModel,
    Offline, StaticEffect,
};
use vantage_vista::VistaCapabilities;

fn columns() -> Vec<FakerColumn> {
    vec![
        FakerColumn {
            name: "id".into(),
            ty: "string".into(),
            flags: vec!["id".into()],
        },
        FakerColumn {
            name: "name".into(),
            ty: "string".into(),
            flags: vec![],
        },
        FakerColumn {
            name: "surname".into(),
            ty: "string".into(),
            flags: vec![],
        },
        FakerColumn {
            name: "age".into(),
            ty: "int".into(),
            flags: vec![],
        },
        FakerColumn {
            name: "balance".into(),
            ty: "money".into(),
            flags: vec![],
        },
    ]
}

fn windowed_caps() -> VistaCapabilities {
    VistaCapabilities {
        can_count: true,
        can_fetch_window: true,
        can_order: true,
        ..VistaCapabilities::default()
    }
}

fn shaped(count: usize, shape: BackendShape) -> FakerTable {
    FakerTable::build_shaped(
        "shaped",
        columns(),
        "id",
        Box::new(StaticEffect { count }),
        shape,
    )
}

#[tokio::test]
async fn advertised_window_serves_and_counts_with_total() {
    let table = shaped(
        30,
        BackendShape {
            capabilities: windowed_caps(),
            seed: Some(1),
            ..BackendShape::default()
        },
    );

    let (rows, total) = table.vista.fetch_window_counted(5, 10).await.unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(total, Some(30));

    // Windows tile without overlap when no skew is configured.
    let (next, _) = table.vista.fetch_window_counted(15, 10).await.unwrap();
    assert!(rows.iter().all(|(id, _)| next.iter().all(|(n, _)| n != id)));
}

#[tokio::test]
async fn unadvertised_operations_refuse_as_unsupported() {
    let table = shaped(
        10,
        BackendShape {
            capabilities: windowed_caps(), // no fetch_page, no fetch_next, no search
            seed: Some(1),
            ..BackendShape::default()
        },
    );

    let page = table.vista.fetch_page(1).await;
    assert!(page.is_err(), "fetch_page must refuse when not advertised");
    let next = table.vista.fetch_next(None).await;
    assert!(next.is_err(), "fetch_next must refuse when not advertised");

    let caps = table.vista.capabilities();
    assert!(!caps.can_search && !caps.can_fetch_page && !caps.can_fetch_next);
}

#[tokio::test]
async fn cursor_pagination_walks_the_whole_set_in_fixed_pages() {
    let table = shaped(
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
        let (rows, next) = table.vista.fetch_next(token).await.unwrap();
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
    let table = shaped(
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

    let (_, token) = table.vista.fetch_next(None).await.unwrap();
    let token = token.expect("more pages exist");

    tokio::time::advance(Duration::from_secs(31)).await;
    let err = table.vista.fetch_next(Some(token)).await.unwrap_err();
    assert!(
        err.to_string().contains("expired"),
        "expected token expiry, got: {err}"
    );
}

#[tokio::test(start_paused = true)]
async fn latency_is_paid_per_operation_class() {
    let table = shaped(
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
    let fetched = table.vista.fetch_window(0, 5);
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
    let table = shaped(
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
    assert!(table.vista.fetch_window(0, 5).await.is_ok());

    // t=55s: inside the final 10s of the period — down.
    tokio::time::advance(Duration::from_secs(55)).await;
    let err = table.vista.fetch_window(0, 5).await.unwrap_err();
    assert!(err.to_string().contains("offline"), "got: {err}");

    // t=65s: next period, online again.
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(table.vista.fetch_window(0, 5).await.is_ok());
}

#[tokio::test]
async fn error_rate_one_fails_everything_and_totals_lie() {
    let table = shaped(
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
    assert!(table.vista.fetch_window(0, 5).await.is_err());
    assert!(table.vista.list_values().await.is_err());

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
    assert_eq!(honest_free.vista.get_count().await.unwrap(), 7);
}

#[tokio::test]
async fn extra_fields_ride_along_undeclared() {
    let table = shaped(
        3,
        BackendShape {
            extra_fields: Some(ExtraFields {
                count: 50,
                size: 1000,
            }),
            seed: Some(6),
            ..BackendShape::default()
        },
    );
    let rows = table.vista.list_values().await.unwrap();
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
    let shape = || BackendShape {
        capabilities: windowed_caps(),
        weirdness: 0.2,
        seed: Some(42),
        ..BackendShape::default()
    };
    let a = shaped(25, shape()).vista.list_values().await.unwrap();
    let b = shaped(25, shape()).vista.list_values().await.unwrap();
    assert_eq!(a, b, "same seed, same rows — a scenario replays identically");
}

#[tokio::test]
async fn search_gates_and_narrows_when_advertised() {
    let mut caps = windowed_caps();
    caps.can_search = true;
    let table = shaped(
        40,
        BackendShape {
            capabilities: caps,
            seed: Some(7),
            ..BackendShape::default()
        },
    );

    let all = table.vista.list_values().await.unwrap();
    // Pick a needle from a real row so the search must hit at least once.
    let needle = all
        .values()
        .find_map(|r| match r.get("name") {
            Some(CborValue::Text(s)) if s.len() >= 3 => Some(s[..3].to_string()),
            _ => None,
        })
        .expect("a generated name to search for");

    let mut vista = table.vista;
    vista.add_search(&needle).unwrap();
    let narrowed = vista.list_values().await.unwrap();
    assert!(!narrowed.is_empty());
    assert!(narrowed.len() <= all.len());
}
