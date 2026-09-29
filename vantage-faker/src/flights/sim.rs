//! The fleet: plans flights and keeps the departure board and the sky full.
//!
//! [`FlightSim`] knows nothing about rows or real time. It owns the seeded rng
//! that draws routes, types and schedules, and on every
//! [`top_up`](FlightSim::top_up) schedules new departures so `board` flights
//! wait on the ground and roughly `fleet` are in the air.

use fake::rand::rngs::StdRng;
use fake::rand::{RngExt as _, SeedableRng as _};
use indexmap::IndexMap;

use super::aircraft::{CARRIERS, types_for};
use super::airports::{AIRPORTS, airport};
use super::flight::{BOARDING_S, Flight, Phase, Profile};
use crate::generator::expand_pattern;

/// Longest airborne time the size-biased initial draw expects, seconds.
const MAX_AIRBORNE_S: f64 = 20.0 * 3600.0;
/// Routes sampled to estimate the mean airborne time.
const ESTIMATE_SAMPLES: usize = 64;
/// Chance a flight is delayed, and the delay range in minutes.
const DELAY_CHANCE: f64 = 0.15;
const DELAY_MINUTES: std::ops::RangeInclusive<u32> = 5..=45;

pub struct FlightSim {
    fleet: usize,
    board: usize,
    rng: StdRng,
    /// Active flights by id, in creation order. Landed ones stay until
    /// [`remove`](Self::remove)d.
    pub flights: IndexMap<String, Flight>,
    next_seq: u64,
    /// Scheduled (pre-delay) departure of the newest flight on the board.
    last_scheduled: f64,
    /// Mean take-off-to-touchdown time over the route mix, seconds.
    mean_airborne_s: f64,
}

impl FlightSim {
    /// A sim whose sky, at sim time `t0`, already holds `fleet` flights at
    /// random points of their journey, with `board` departures scheduled.
    pub fn new(fleet: usize, board: usize, seed: Option<u64>, t0: f64) -> Self {
        let rng = match seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => crate::value_gen::entropy_rng(),
        };
        let mut sim = Self {
            fleet,
            board,
            rng,
            flights: IndexMap::new(),
            next_seq: 0,
            last_scheduled: t0,
            mean_airborne_s: 0.0,
        };
        let sampled: f64 = (0..ESTIMATE_SAMPLES)
            .map(|_| sim.draw(0.0).airborne_s())
            .sum();
        sim.mean_airborne_s = sampled / ESTIMATE_SAMPLES as f64;

        // Size-biased draw (accept in proportion to flight length) so the
        // initial sky has the mix a steady state would: long flights are aloft
        // for longer, so more of them are in the air at any moment.
        while sim.flights.len() < fleet {
            let mut f = sim.draw(0.0);
            if sim.rng.random_range(0.0..1.0) > f.airborne_s() / MAX_AIRBORNE_S {
                continue;
            }
            let at = sim.rng.random_range(0.0..1.0) * f.airborne_s();
            f.scheduled = t0 - (f.takeoff() - f.scheduled) - at;
            sim.admit(f);
        }
        sim.top_up(t0);
        sim
    }

    /// Schedule departures until `board` flights are waiting to leave at sim
    /// time `t`. Returns the ids of the flights added.
    ///
    /// One scan counts the waiting and in-air flights; every added flight
    /// departs after `t`, so it only bumps the waiting count.
    pub fn top_up(&mut self, t: f64) -> Vec<String> {
        let (mut waiting, mut in_air) = (0usize, 0u32);
        for f in self.flights.values() {
            match f.phase_at(t) {
                p if p <= Phase::Boarding => waiting += 1,
                Phase::Landed => {}
                _ => in_air += 1,
            }
        }
        let fleet = self.fleet.max(1) as f64;
        let pressure = (f64::from(in_air) / fleet).clamp(0.5, 2.0);
        let gap = self.mean_airborne_s / fleet * pressure;
        let mut added = Vec::new();
        while waiting < self.board {
            // Never schedule into the past, or inside the boarding window
            // of a flight that should still appear as Scheduled.
            let base = self.last_scheduled.max(t + BOARDING_S / 2.0);
            self.last_scheduled = base + gap;
            let f = self.draw(self.last_scheduled);
            added.push(self.admit(f));
            waiting += 1;
        }
        added
    }

    /// Drop a flight (after its landed row has been retired).
    pub fn remove(&mut self, id: &str) {
        self.flights.swap_remove(id);
    }

    #[cfg(test)]
    pub fn count(&self, t: f64, pred: impl Fn(Phase) -> bool) -> usize {
        self.flights
            .values()
            .filter(|f| pred(f.phase_at(t)))
            .count()
    }

    fn admit(&mut self, mut f: Flight) -> String {
        f.seq = self.next_seq;
        f.id = format!("{:08}", self.next_seq);
        self.next_seq += 1;
        let id = f.id.clone();
        self.flights.insert(id.clone(), f);
        id
    }

    /// Draw a flight scheduled to leave at `scheduled`: a carrier, a route
    /// touching its hub, a type with the range, and the paperwork.
    fn draw(&mut self, scheduled: f64) -> Flight {
        let rng = &mut self.rng;
        loop {
            let carrier = &CARRIERS[rng.random_range(0..CARRIERS.len())];
            let hub = airport(carrier.hub).expect("carrier hubs are built-in airports");
            let other = &AIRPORTS[rng.random_range(0..AIRPORTS.len())];
            if other.iata == hub.iata {
                continue;
            }
            let (origin, destination) = if rng.random_bool(0.5) {
                (hub, other)
            } else {
                (other, hub)
            };
            let nm = super::airports::distance_nm(origin, destination);
            let types = types_for(nm);
            if types.is_empty() {
                continue;
            }
            let aircraft = types[rng.random_range(0..types.len())];
            let seats = aircraft.seats;
            let delay_minutes = if rng.random_bool(DELAY_CHANCE) {
                rng.random_range(DELAY_MINUTES)
            } else {
                0
            };
            let gate = format!(
                "{}{}",
                char::from(b'A' + rng.random_range(0..6u8)),
                rng.random_range(1..=60u32)
            );
            return Flight {
                id: String::new(),
                seq: 0,
                flight_no: format!("{}{}", carrier.code, rng.random_range(10..=2999u32)),
                carrier: carrier.code,
                aircraft,
                registration: expand_pattern(rng, carrier.registration),
                origin,
                destination,
                gate,
                pax: rng.random_range(seats * 13 / 20..=seats),
                scheduled,
                delay_minutes,
                distance_nm: 0.0,
                peak_ft: 0.0,
                profile: Profile::default(),
                wobble: rng.random_range(0.0..std::f64::consts::TAU),
            }
            .plan();
        }
    }
}
