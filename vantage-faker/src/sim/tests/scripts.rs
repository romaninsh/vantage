//! The example scripts in `examples/sims/` run and produce plausible rows.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::generator::parse_when;

const FLIGHT: &str = include_str!("../../../examples/sims/flight.rhai");
const SHIPMENT: &str = include_str!("../../../examples/sims/shipment.rhai");

fn wall(rec: &Record<CborValue>, col: &str) -> f64 {
    parse_when(&text(rec, col), 0).unwrap_or_else(|| panic!("{col} is not a time")) as f64
}

#[test]
fn flight_script_opens_a_board_mid_flight() {
    let store = store_with(&["flights"]);
    let flights = store.table("flights");
    let def = SimDef::new("flight", "flights", FLIGHT)
        .with_spawn(0, 0.2, 60)
        .with_clock(10.0)
        .with_warm(Duration::from_secs(4 * 3600));
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .seed(3)
        .start()
        .unwrap();
    engine.settle();

    let rows = rows(&flights);
    let mut phases: HashMap<String, usize> = HashMap::new();
    for row in &rows {
        *phases.entry(text(row, "phase")).or_default() += 1;
        let progress = num(row, "progress");
        assert!((0.0..=100.0).contains(&progress), "{progress}");
        assert!(text(row, "flight_time").ends_with('m'));
        let phase = text(row, "phase");
        if phase == "Cruising" {
            assert_eq!(num(row, "altitude_ft"), 38000.0);
            // Under 20 sim hours to go is under two real hours at 10x.
            let eta = wall(row, "eta");
            assert!(eta > T0 as f64 && eta < (T0 + 7200) as f64, "{eta}");
            assert!(wall(row, "actual_departure") < T0 as f64);
        }
    }
    assert!(rows.len() >= 30, "{} flights: {phases:?}", rows.len());
    assert!(
        phases.get("Cruising").copied().unwrap_or(0) >= 20,
        "{phases:?}"
    );

    // Live: ten sim minutes move the cruising flights on.
    let before: HashMap<String, f64> = rows
        .iter()
        .map(|r| (text(r, "id"), num(r, "progress")))
        .collect();
    run_for(&engine, 60, 3);
    let moved = super::rows(&flights)
        .iter()
        .filter(|r| {
            before
                .get(&text(r, "id"))
                .is_some_and(|p| num(r, "progress") > *p)
        })
        .count();
    assert!(moved >= 20, "{moved} moved");
}

/// Demo scale: about 200 flights over a 12 h warm start. Run with
/// `cargo test --release --all-features -- --ignored warm_start_at_demo_scale --nocapture`.
#[test]
#[ignore = "benchmark; run in release"]
fn warm_start_at_demo_scale() {
    let store = store_with(&["flights"]);
    let flights = store.table("flights");
    let def = SimDef::new("flight", "flights", FLIGHT)
        .with_spawn(0, 0.3, 260)
        .with_clock(10.0)
        .with_warm(Duration::from_secs(12 * 3600));
    let t = std::time::Instant::now();
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .seed(5)
        .start()
        .unwrap();
    let took = t.elapsed();
    eprintln!("warm start: {took:?}, {} flights", rows(&flights).len());
    assert!(engine.live() >= 150, "{} live", engine.live());
}

#[test]
fn shipment_script_writes_shipments_and_their_tracking_events() {
    let store = store_with(&["shipment", "tracking_event"]);
    let (shipments, events) = (store.table("shipment"), store.table("tracking_event"));
    let def = SimDef::new("shipment", "shipment", SHIPMENT)
        .with_spawn(5, 0.05, 200)
        .with_clock(60.0)
        .with_warm(Duration::from_secs(2 * 86_400));
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .seed(4)
        .start()
        .unwrap();
    engine.settle();

    let ships = rows(&shipments);
    assert!(ships.len() >= 50, "{} shipments", ships.len());
    let statuses = [
        "Booked",
        "Picked up",
        "In transit",
        "Exception",
        "Out for delivery",
        "Delivered",
    ];
    let mut seen = HashSet::new();
    for s in &ships {
        let status = text(s, "status");
        assert!(statuses.contains(&status.as_str()), "{status}");
        seen.insert(status);
        assert!(text(s, "tracking_no").starts_with("SH-"));
        assert!(matches!(s.get("late"), Some(CborValue::Bool(_))));
        let (lat, lon) = (num(s, "lat"), num(s, "lon"));
        assert!((35.0..60.0).contains(&lat) && (-10.0..25.0).contains(&lon));
    }
    assert!(
        seen.contains("Delivered") && seen.contains("In transit"),
        "{seen:?}"
    );

    let ids: HashSet<String> = ships.iter().map(|s| text(s, "id")).collect();
    let mut tracked = HashSet::new();
    for e in rows(&events) {
        let owner = text(&e, "shipment_id");
        assert!(ids.contains(&owner), "event for a missing shipment {owner}");
        tracked.insert(owner);
    }
    assert_eq!(tracked, ids, "every shipment has tracking events");
}
