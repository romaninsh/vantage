//! `builtin:flight`: one sim per aircraft, filling every flight column from
//! the departure board until the landed row is deleted.

use super::*;

const COLUMNS: &[&str] = &[
    "id",
    "flight_no",
    "carrier",
    "aircraft_type",
    "registration",
    "origin",
    "origin_city",
    "destination",
    "destination_city",
    "phase",
    "progress",
    "altitude_ft",
    "ground_speed_kts",
    "heading",
    "distance_total_nm",
    "distance_remaining_nm",
    "scheduled_departure",
    "actual_departure",
    "eta",
    "flight_time",
    "flight_time_min",
    "delay_minutes",
    "gate",
    "pax",
    "seats",
    "lat",
    "lon",
    "board_order",
];

fn flights(args: serde_json::Value) -> (SimEngine, MemoryTableHandle) {
    let store = store_with(&["flight"]);
    let def = SimDef::new(
        "f",
        "flight",
        crate::sim::builtin::builtin("flight").unwrap(),
    )
    .with_spawn(20, 0.0, 20)
    .with_clock(10.0)
    .with_args(args);
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .seed(3)
        .start()
        .unwrap();
    (engine, store.table("flight"))
}

#[test]
fn every_flight_column_is_written() {
    let (engine, t) = flights(serde_json::json!({}));
    run_for(&engine, 60, 5);
    let row = t.get(&t.ids()[0]).unwrap();
    for c in COLUMNS {
        assert!(row.get(*c).is_some(), "missing {c}");
    }
    assert!(matches!(
        row.get("board_order"),
        Some(CborValue::Integer(_))
    ));
}

#[test]
fn flights_land_and_leave() {
    let (engine, t) = flights(serde_json::json!({ "landed_retention": 60 }));
    let mut saw_landed = false;
    for _ in 0..3000 {
        run_for(&engine, 10, 10);
        if t.ids()
            .iter()
            .filter_map(|id| t.get(id))
            .any(|r| text(&r, "phase") == "Landed")
        {
            saw_landed = true;
        }
        if t.is_empty() {
            break;
        }
    }
    assert!(saw_landed);
    assert!(t.is_empty(), "landed flights are deleted after retention");
    assert_eq!(engine.stats().errored, 0);
}
