//! The sim's column vocabulary: which names it fills and with what.

use ciborium::Value as CborValue;

use super::flight::{Flight, Phase, State};
use crate::generator::rfc3339;

/// Column names the flight sim fills. Any other declared column is an extra,
/// generated once by [`ValueGen`](crate::ValueGen) when the row is created.
pub const FLIGHT_COLUMNS: &[&str] = &[
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

/// Whether the sim fills a column called `name`.
pub fn is_flight_column(name: &str) -> bool {
    FLIGHT_COLUMNS.contains(&name)
}

fn int(v: f64) -> CborValue {
    CborValue::Integer((v.round() as i64).into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

/// Round to `places` decimals, so a cell only changes when its shown value does.
fn float(v: f64, places: i32) -> CborValue {
    let k = 10f64.powi(places);
    CborValue::Float((v * k).round() / k)
}

/// Maps sim-clock instants onto the wall clock. The sim clock equals the
/// wall clock at `t0` and runs `scale` times faster.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Unix seconds where both clocks meet.
    pub t0: f64,
    /// Sim seconds per real second.
    pub scale: f64,
}

impl Clock {
    /// Sim time after `elapsed_real` seconds.
    pub fn sim(&self, elapsed_real: f64) -> f64 {
        self.t0 + elapsed_real * self.scale
    }

    /// The wall-clock instant the sim reaches `sim_secs`: now plus the real
    /// time left until then.
    pub fn wall(&self, sim_secs: f64) -> f64 {
        self.t0 + (sim_secs - self.t0) / self.scale
    }

    fn ts(&self, sim_secs: f64) -> CborValue {
        CborValue::Text(rfc3339(self.wall(sim_secs).round() as i64))
    }
}

/// A sim duration as `11h 10m`, rounded to the minute.
pub fn hours_minutes(secs: f64) -> String {
    let min = (secs / 60.0).round().max(0.0) as u64;
    format!("{}h {}m", min / 60, min % 60)
}

/// Ascending sort key for a departures-then-arrivals board: flights waiting
/// to leave (1000 + minutes to departure), taxiing (2000 + percent of taxi
/// done), airborne (3000 + progress), landed (4000).
pub fn board_order(f: &Flight, s: &State, t: f64) -> f64 {
    let rounded = |v: f64| (v * 10.0).round() / 10.0;
    match s.phase {
        Phase::Scheduled | Phase::Boarding => {
            1000.0 + rounded(((f.departure() - t) / 60.0).clamp(0.0, 999.0))
        }
        Phase::Taxiing => {
            let done = (t - f.departure()) / super::flight::TAXI_S * 100.0;
            2000.0 + rounded(done.clamp(0.0, 100.0))
        }
        Phase::Landed => 4000.0,
        _ => 3000.0 + rounded(f.progress(s)),
    }
}

/// The value of sim column `name` for flight `f` in state `s` at sim time
/// `t`, or `None` if the sim does not know the name. Timestamps are
/// wall-clock instants through `clock`.
pub fn flight_value(f: &Flight, s: &State, t: f64, clock: &Clock, name: &str) -> Option<CborValue> {
    let remaining = f.distance_nm - s.flown_nm;
    Some(match name {
        "id" => text(&f.id),
        "flight_no" => text(&f.flight_no),
        "carrier" => text(f.carrier),
        "aircraft_type" => text(f.aircraft.code),
        "registration" => text(&f.registration),
        "origin" => text(f.origin.iata),
        "origin_city" => text(f.origin.city),
        "destination" => text(f.destination.iata),
        "destination_city" => text(f.destination.city),
        "phase" => text(s.phase.label()),
        "progress" => float(f.progress(s), 1),
        "altitude_ft" => int((s.altitude_ft / 100.0).round() * 100.0),
        "ground_speed_kts" => int(s.ground_speed_kts),
        "heading" => int(s.heading.round() % 360.0),
        "distance_total_nm" => int(f.distance_nm),
        "distance_remaining_nm" => int(remaining.max(0.0)),
        "scheduled_departure" => clock.ts(f.scheduled),
        "actual_departure" if s.phase >= Phase::Taxiing => clock.ts(f.departure()),
        "actual_departure" => CborValue::Null,
        "eta" => clock.ts(f.eta()),
        "flight_time" => text(&hours_minutes(f.flight_time_s())),
        "flight_time_min" => int(f.flight_time_s() / 60.0),
        "delay_minutes" => int(f64::from(f.delay_minutes)),
        "gate" => text(&f.gate),
        "pax" => int(f64::from(f.pax)),
        "seats" => int(f64::from(f.aircraft.seats)),
        "lat" => float(s.lat, 4),
        "lon" => float(s.lon, 4),
        "board_order" => float(board_order(f, s, t), 1),
        _ => return None,
    })
}
