use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_diorama::ChangeEvent;
use vantage_vista::mocks::MockShell;

use super::airports::{airport, distance_nm};
use super::flight::Phase;
use super::sim::FlightSim;
use super::*;
use crate::{ColumnGen, FakerColumn};

const T0: f64 = 1_790_000_000.0;

fn columns(names: &[&str]) -> Vec<FakerColumn> {
    names
        .iter()
        .map(|n| FakerColumn::new(*n, "string"))
        .collect()
}

fn ctx(cols: Vec<FakerColumn>) -> (Arc<FakerCtx>, broadcast::Receiver<ChangeEvent>) {
    let (tx, rx) = broadcast::channel(1 << 16);
    (
        Arc::new(FakerCtx::new(MockShell::new(), tx, cols, "id".into())),
        rx,
    )
}

fn effect(fleet: usize, board: usize, seed: u64) -> FlightsEffect {
    FlightsEffect::new(FlightsConfig {
        fleet,
        board,
        seed: Some(seed),
        ..FlightsConfig::default()
    })
    .with_start(UNIX_EPOCH + Duration::from_secs(T0 as u64))
}

fn text(rec: &vantage_types::Record<CborValue>, col: &str) -> String {
    match rec.get(col) {
        Some(CborValue::Text(s)) => s.clone(),
        other => panic!("{col} is not text: {other:?}"),
    }
}

fn num(rec: &vantage_types::Record<CborValue>, col: &str) -> f64 {
    match rec.get(col) {
        Some(CborValue::Float(f)) => *f,
        Some(CborValue::Integer(i)) => i128::from(*i) as f64,
        other => panic!("{col} is not a number: {other:?}"),
    }
}

fn first_waiting(sim: &FlightSim) -> &super::flight::Flight {
    sim.flights
        .values()
        .find(|f| f.phase_at(T0) == Phase::Scheduled)
        .expect("the board holds a scheduled flight")
}

#[test]
fn great_circle_distance_is_realistic() {
    let nm = distance_nm(airport("LHR").unwrap(), airport("JFK").unwrap());
    assert!((2950.0..3050.0).contains(&nm), "LHR-JFK {nm} nm");
}

#[test]
fn lifecycle_runs_in_order_with_monotonic_progress() {
    let sim = FlightSim::new(10, 4, Some(1), T0);
    let f = first_waiting(&sim);
    let (mut phases, mut last_progress) = (vec![], 0.0);
    let mut t = T0;
    while t <= f.eta() + 120.0 {
        let s = f.state_at(t);
        if phases.last() != Some(&s.phase) {
            phases.push(s.phase);
        }
        let p = f.progress(&s);
        assert!(p >= last_progress, "progress fell {last_progress} -> {p}");
        last_progress = p;
        t += 30.0;
    }
    use Phase::*;
    let expected = [
        Scheduled, Boarding, Taxiing, Takeoff, Climbing, Cruising, Descending, Landing, Landed,
    ];
    assert_eq!(phases, expected);
    assert_eq!(last_progress, 100.0);
}

#[test]
fn altitude_is_zero_on_the_ground_and_peaks_in_cruise() {
    let sim = FlightSim::new(10, 4, Some(2), T0);
    let f = first_waiting(&sim);
    let mut peak = (0.0, Phase::Scheduled);
    let mut t = T0;
    while t <= f.eta() + 120.0 {
        let s = f.state_at(t);
        if !s.phase.airborne() {
            assert_eq!(s.altitude_ft, 0.0, "{:?} above ground", s.phase);
        }
        if s.altitude_ft > peak.0 {
            peak = (s.altitude_ft, s.phase);
        }
        t += 30.0;
    }
    assert_eq!(peak.1, Phase::Cruising);
    assert!(peak.0 > 10_000.0, "peak {}", peak.0);
}

#[test]
fn in_air_count_stays_near_fleet() {
    for seed in 0..5 {
        assert_fleet_holds(seed);
    }
}

fn assert_fleet_holds(seed: u64) {
    let fleet = 40;
    let mut sim = FlightSim::new(fleet, 8, Some(seed), T0);
    let in_air =
        |sim: &FlightSim, t| sim.count(t, |p| (Phase::Taxiing..Phase::Landed).contains(&p));
    for minute in 0..48 * 60 {
        let t = T0 + f64::from(minute) * 60.0;
        sim.top_up(t);
        let landed: Vec<String> = sim
            .flights
            .iter()
            .filter(|(_, f)| f.phase_at(t) == Phase::Landed)
            .map(|(id, _)| id.clone())
            .collect();
        landed.iter().for_each(|id| sim.remove(id));
        let n = in_air(&sim, t);
        assert!(
            (28..=52).contains(&n),
            "seed {seed}, minute {minute}: {n} in the air"
        );
        assert_eq!(sim.count(t, |p| p <= Phase::Boarding), 8);
    }
}

#[test]
fn landed_rows_freeze_then_go_after_retention() {
    let (ctx, mut rx) = ctx(columns(&["id", "phase", "progress", "board_order"]));
    let fx = effect(6, 2, 4);
    fx.seed(&ctx);
    let mut landed: Option<(String, u64)> = None;
    for sec in 1..=3600u64 {
        fx.advance(&ctx, Duration::from_secs(sec));
        while let Ok(ev) = rx.try_recv() {
            match (&ev, &landed) {
                (ChangeEvent::Updated { id, new: Some(rec) }, None) => {
                    if text(rec, "phase") == "Landed" {
                        landed = Some((id.clone(), sec));
                    }
                }
                (ChangeEvent::Updated { id, .. }, Some((watched, _))) => {
                    assert_ne!(id, watched, "a landed row was patched");
                }
                (ChangeEvent::Deleted { id }, Some((watched, at))) if id == watched => {
                    assert_eq!(sec - at, 60, "deleted after the retention");
                    assert!(ctx.get_record(id).is_none());
                    return;
                }
                _ => {}
            }
        }
    }
    panic!("no flight landed and retired within an hour ({landed:?})");
}

#[test]
fn board_order_sorts_departures_then_air_by_progress_then_landed() {
    let (ctx, _rx) = ctx(columns(&["id", "phase", "progress", "board_order"]));
    let fx = effect(30, 8, 5);
    fx.seed(&ctx);
    for sec in 1..=300 {
        fx.advance(&ctx, Duration::from_secs(sec));
    }
    let mut rows: Vec<_> = ctx
        .record_ids()
        .iter()
        .map(|id| ctx.get_record(id).unwrap())
        .collect();
    rows.sort_by(|a, b| num(a, "board_order").total_cmp(&num(b, "board_order")));
    let rank = |r: &vantage_types::Record<CborValue>| match text(r, "phase").as_str() {
        "Scheduled" | "Boarding" => 0,
        "Taxiing" => 1,
        "Landed" => 3,
        _ => 2,
    };
    let ranks: Vec<_> = rows.iter().map(rank).collect();
    assert!(ranks.is_sorted(), "{ranks:?}");
    assert!(
        ranks.contains(&0) && ranks.contains(&2) && ranks.contains(&3),
        "{ranks:?}"
    );
    let air: Vec<f64> = rows
        .iter()
        .filter(|r| rank(r) == 2)
        .map(|r| num(r, "progress"))
        .collect();
    assert!(air.is_sorted(), "airborne not by progress: {air:?}");
}

#[test]
fn column_subset_with_extras() {
    let mut cols = columns(&["flight_no", "id", "remarks", "phase", "passenger_name"]);
    cols[2] = FakerColumn::new("remarks", "string").with_generator(ColumnGen::Pick {
        values: vec!["ok".into()],
        weights: None,
    });
    let (ctx, _rx) = ctx(cols);
    let fx = effect(5, 2, 6);
    fx.seed(&ctx);
    let id = ctx.record_ids()[0].clone();
    let before = ctx.get_record(&id).unwrap();
    let keys: Vec<&str> = before.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["flight_no", "id", "remarks", "phase", "passenger_name"]
    );
    assert_eq!(text(&before, "remarks"), "ok");
    assert!(!text(&before, "passenger_name").is_empty());
    for sec in 1..=120 {
        fx.advance(&ctx, Duration::from_secs(sec));
    }
    if let Some(after) = ctx.get_record(&id) {
        assert_eq!(after.get("passenger_name"), before.get("passenger_name"));
        assert_eq!(after.get("flight_no"), before.get("flight_no"));
    }
}

#[test]
fn seeded_runs_are_identical() {
    let snapshot = |seed| {
        let (ctx, _rx) = ctx(columns(FLIGHT_COLUMNS));
        let fx = effect(20, 6, seed);
        fx.seed(&ctx);
        for sec in 1..=90 {
            fx.advance(&ctx, Duration::from_secs(sec));
        }
        let mut ids = ctx.record_ids();
        ids.sort();
        ids.iter()
            .map(|id| ctx.get_record(id).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(snapshot(7), snapshot(7));
    assert_ne!(snapshot(7), snapshot(8));
}

#[test]
fn timestamps_are_wall_clock_and_flight_time_is_sim() {
    let (ctx, _rx) = ctx(columns(&[
        "id",
        "eta",
        "scheduled_departure",
        "flight_time",
        "flight_time_min",
    ]));
    let fx = effect(10, 4, 11);
    fx.seed(&ctx);
    let state = fx.state.lock().unwrap();
    let sim = &state.as_ref().unwrap().sim;
    let scale = FlightsConfig::default().time_scale;
    for (id, f) in &sim.flights {
        let rec = ctx.get_record(id).unwrap();
        let wall_eta = T0 + (f.eta() - T0) / scale;
        let expected = crate::generator::rfc3339(wall_eta.round() as i64);
        assert_eq!(text(&rec, "eta"), expected);
        let minutes = num(&rec, "flight_time_min");
        assert_eq!(minutes, (f.flight_time_s() / 60.0).round());
        let m = minutes as u64;
        assert_eq!(
            text(&rec, "flight_time"),
            format!("{}h {}m", m / 60, m % 60)
        );
    }
    assert_eq!(
        super::columns::hours_minutes(11.0 * 3600.0 + 600.0),
        "11h 10m"
    );
}

#[tokio::test]
async fn live_table_broadcasts_patches() {
    let table = crate::FakerTable::build(
        "flights",
        columns(&["id", "flight_no", "progress"]),
        "id",
        Box::new(FlightsEffect::new(FlightsConfig {
            fleet: 5,
            board: 2,
            tick: Duration::from_millis(10),
            time_scale: 600.0,
            ..FlightsConfig::default()
        })),
    );
    let mut rx = table.events.subscribe();
    let got = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(ChangeEvent::Updated { .. }) = rx.recv().await {
                return true;
            }
        }
    })
    .await;
    assert!(got.is_ok(), "no Updated within 2s");
}

mod limits;
mod traffic;
