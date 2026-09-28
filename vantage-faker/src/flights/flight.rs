//! One flight: its fixed plan and its state at any sim instant.
//!
//! Times are sim-clock unix seconds (`f64`). Departure means off-block
//! (pushback); take-off follows a fixed taxi, then a climb, cruise and
//! descent whose durations are fixed when the flight is planned, so progress
//! and ETA are pure functions of the sim time.

use super::aircraft::Aircraft;
use super::airports::{Airport, bearing, distance_nm, interpolate};

/// Boarding opens this long before departure.
pub const BOARDING_S: f64 = 30.0 * 60.0;
/// Pushback to take-off.
pub const TAXI_S: f64 = 12.0 * 60.0;
/// Take-off roll and initial climb: the first part of the climb.
pub const TAKEOFF_S: f64 = 5.0 * 60.0;
/// Final approach and touchdown: the last part of the descent.
pub const LANDING_S: f64 = 5.0 * 60.0;
/// Climb and descent rates, feet per minute.
const CLIMB_FPM: f64 = 2_000.0;
const DESCENT_FPM: f64 = 1_500.0;
/// Average speed over climb and descent as a fraction of cruise speed.
const TRANSITION_SPEED: f64 = 0.65;

/// Where a flight is in its life. Ordered: a flight only moves forward.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Scheduled,
    Boarding,
    Taxiing,
    Takeoff,
    Climbing,
    Cruising,
    Descending,
    Landing,
    Landed,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "Scheduled",
            Self::Boarding => "Boarding",
            Self::Taxiing => "Taxiing",
            Self::Takeoff => "Takeoff",
            Self::Climbing => "Climbing",
            Self::Cruising => "Cruising",
            Self::Descending => "Descending",
            Self::Landing => "Landing",
            Self::Landed => "Landed",
        }
    }

    pub fn airborne(self) -> bool {
        (Self::Takeoff..=Self::Landing).contains(&self)
    }
}

/// A planned flight. Everything here is fixed at creation.
#[derive(Debug)]
pub struct Flight {
    pub id: String,
    /// Creation order; the `seq` extra columns are generated at.
    pub seq: u64,
    pub flight_no: String,
    pub carrier: &'static str,
    pub aircraft: &'static Aircraft,
    pub registration: String,
    pub origin: &'static Airport,
    pub destination: &'static Airport,
    pub gate: String,
    pub pax: u32,
    pub scheduled: f64,
    pub delay_minutes: u32,
    pub distance_nm: f64,
    /// Cruise altitude actually reached (lower on short hops).
    pub peak_ft: f64,
    /// Filled by [`plan`](Self::plan).
    pub profile: Profile,
    /// Offset of the ground-speed wobble, so flights do not wobble in step.
    pub wobble: f64,
}

/// Segment durations (seconds) and distances (nm) of the flight profile.
#[derive(Clone, Copy, Debug, Default)]
pub struct Profile {
    climb_s: f64,
    cruise_s: f64,
    descent_s: f64,
    climb_nm: f64,
    descent_nm: f64,
}

/// What a flight looks like at one sim instant.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub phase: Phase,
    pub flown_nm: f64,
    pub altitude_ft: f64,
    pub ground_speed_kts: f64,
    pub heading: f64,
    pub lat: f64,
    pub lon: f64,
}

impl Flight {
    /// Plan the climb/cruise/descent profile for the route and type. Short
    /// hops scale the climb and descent down (and the peak altitude with
    /// them) so they fit in 90% of the distance.
    pub fn plan(mut self) -> Self {
        let v = self.aircraft.cruise_kts;
        let h = self.aircraft.cruise_ft;
        self.distance_nm = distance_nm(self.origin, self.destination);
        let climb_s = h / CLIMB_FPM * 60.0;
        let descent_s = h / DESCENT_FPM * 60.0;
        let nm = |s: f64| TRANSITION_SPEED * v * s / 3600.0;
        let k = (0.9 * self.distance_nm / (nm(climb_s) + nm(descent_s))).min(1.0);
        self.peak_ft = h * k;
        let (climb_s, descent_s) = (climb_s * k, descent_s * k);
        let (climb_nm, descent_nm) = (nm(climb_s), nm(descent_s));
        self.profile = Profile {
            climb_s,
            cruise_s: (self.distance_nm - climb_nm - descent_nm) / v * 3600.0,
            descent_s,
            climb_nm,
            descent_nm,
        };
        self
    }

    /// Off-block time: scheduled plus delay.
    pub fn departure(&self) -> f64 {
        self.scheduled + f64::from(self.delay_minutes) * 60.0
    }

    pub fn takeoff(&self) -> f64 {
        self.departure() + TAXI_S
    }

    /// Take-off to touchdown.
    pub fn airborne_s(&self) -> f64 {
        let p = &self.profile;
        p.climb_s + p.cruise_s + p.descent_s
    }

    /// Touchdown time.
    pub fn eta(&self) -> f64 {
        self.takeoff() + self.airborne_s()
    }

    pub fn phase_at(&self, t: f64) -> Phase {
        let dep = self.departure();
        let up = self.takeoff();
        let p = &self.profile;
        let eta = self.eta();
        match t {
            t if t < dep - BOARDING_S => Phase::Scheduled,
            t if t < dep => Phase::Boarding,
            t if t < up => Phase::Taxiing,
            t if t < up + TAKEOFF_S.min(p.climb_s) => Phase::Takeoff,
            t if t < up + p.climb_s => Phase::Climbing,
            t if t < up + p.climb_s + p.cruise_s => Phase::Cruising,
            t if t < eta - LANDING_S.min(p.descent_s) => Phase::Descending,
            t if t < eta => Phase::Landing,
            _ => Phase::Landed,
        }
    }

    /// Off-block to touchdown, sim seconds.
    pub fn flight_time_s(&self) -> f64 {
        self.eta() - self.departure()
    }

    pub fn state_at(&self, t: f64) -> State {
        let phase = self.phase_at(t);
        let v = self.aircraft.cruise_kts;
        let air = t - self.takeoff();
        let p = &self.profile;
        let frac = |x: f64, len: f64| {
            if len > 0.0 {
                (x / len).clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let (flown_nm, altitude_ft, ground_speed_kts) = match phase {
            Phase::Scheduled | Phase::Boarding => (0.0, 0.0, 0.0),
            Phase::Taxiing => (0.0, 0.0, 15.0),
            Phase::Takeoff | Phase::Climbing => {
                let f = frac(air, p.climb_s);
                (f * p.climb_nm, f * self.peak_ft, 160.0 + (v - 160.0) * f)
            }
            Phase::Cruising => {
                let cruised = air - p.climb_s;
                let wobble = 1.0 + 0.03 * (t / 600.0 + self.wobble).sin();
                (p.climb_nm + cruised * v / 3600.0, self.peak_ft, v * wobble)
            }
            Phase::Descending | Phase::Landing => {
                let f = frac(air - p.climb_s - p.cruise_s, p.descent_s);
                let before = self.distance_nm - p.descent_nm;
                let alt = (1.0 - f) * self.peak_ft;
                (before + f * p.descent_nm, alt, v - (v - 150.0) * f)
            }
            Phase::Landed => (self.distance_nm, 0.0, 0.0),
        };
        let flown_nm = flown_nm.min(self.distance_nm);
        let f = if self.distance_nm > 0.0 {
            flown_nm / self.distance_nm
        } else {
            1.0
        };
        let (lat, lon) = interpolate(self.origin, self.destination, f);
        let (dest, orig) = (self.destination, self.origin);
        let heading = if phase == Phase::Landed {
            (bearing(dest.lat, dest.lon, orig.lat, orig.lon) + 180.0) % 360.0
        } else {
            bearing(lat, lon, dest.lat, dest.lon)
        };
        State {
            phase,
            flown_nm,
            altitude_ft,
            ground_speed_kts,
            heading,
            lat,
            lon,
        }
    }

    /// Percent of the distance flown, 0–100.
    pub fn progress(&self, s: &State) -> f64 {
        if self.distance_nm > 0.0 {
            (s.flown_nm / self.distance_nm * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        }
    }
}
