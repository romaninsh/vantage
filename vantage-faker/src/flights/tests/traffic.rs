//! Traffic shape at the default fleet, board and time scale.

use super::*;
use crate::flights::flight::{LANDING_S, TAKEOFF_S};

#[test]
fn takeoff_and_landing_last_at_least_20_real_seconds() {
    let scale = FlightsConfig::default().time_scale;
    assert!(
        LANDING_S / scale >= 20.0,
        "landing {}s real",
        LANDING_S / scale
    );
    assert!(
        TAKEOFF_S / scale >= 20.0,
        "takeoff {}s real",
        TAKEOFF_S / scale
    );
}

#[test]
fn route_mix_is_long_haul() {
    let sim = FlightSim::new(0, 2000, Some(3), T0);
    let hours: Vec<f64> = sim
        .flights
        .values()
        .map(|f| f.airborne_s() / 3600.0)
        .collect();
    let mean = hours.iter().sum::<f64>() / hours.len() as f64;
    assert!((9.5..=11.5).contains(&mean), "mean {mean:.2} h");
    assert!(hours.iter().all(|h| (3.0..=16.0).contains(h)));
}

#[test]
fn one_or_two_landings_and_takeoffs_at_a_time() {
    let cfg = FlightsConfig::default();
    for seed in 0..3 {
        let mut sim = FlightSim::new(cfg.fleet, cfg.board, Some(seed), T0);
        let (mut landing, mut takeoff, mut samples) = (0usize, 0usize, 0usize);
        // Every 3 real seconds over 12 sim hours.
        let step = 3.0 * cfg.time_scale;
        let mut t = T0;
        while t < T0 + 12.0 * 3600.0 {
            sim.top_up(t);
            let landed: Vec<String> = sim
                .flights
                .iter()
                .filter(|(_, f)| f.phase_at(t) == Phase::Landed)
                .map(|(id, _)| id.clone())
                .collect();
            landed.iter().for_each(|id| sim.remove(id));
            landing += sim.count(t, |p| p == Phase::Landing);
            takeoff += sim.count(t, |p| p == Phase::Takeoff);
            samples += 1;
            t += step;
        }
        let landing = landing as f64 / samples as f64;
        let takeoff = takeoff as f64 / samples as f64;
        assert!(
            (0.5..=3.0).contains(&landing),
            "seed {seed}: {landing:.2} landing"
        );
        assert!(
            (0.5..=3.0).contains(&takeoff),
            "seed {seed}: {takeoff:.2} taking off"
        );
    }
}
