//! Flights — a live fleet taking off, flying and landing, as one table.
//!
//! [`FlightsEffect`] is a [`FakerEffect`]: build it into a table with
//! [`FakerTable::build`](crate::FakerTable::build) like any other effect. Each
//! row is one flight moving through [`Phase`]s: `Scheduled → Boarding →
//! Taxiing → Climbing → Cruising → Descending → Landed`. The table opens with
//! `fleet` flights already in the air at random points of their journeys and
//! `board` departures waiting; new departures keep being scheduled so the
//! board stays full and the in-air count hovers around `fleet`.
//!
//! Flights run between ~35 built-in airports along great circles, flown by a
//! small set of aircraft types at their cruise speeds with a climb, cruise and
//! descent profile. See [`FLIGHT_COLUMNS`] for the columns the sim fills;
//! every other declared column is generated once when the row is created.
//!
//! **Clocks.** The sim clock starts at the wall-clock time the table is built
//! and runs `time_scale` times faster than real time. Every timestamp column
//! (`scheduled_departure`, `actual_departure`, `eta`) is RFC 3339 UTC on the
//! sim clock. Landed-row retention and the tick run on real time.
//!
//! **Deltas.** New flights broadcast `Inserted`; each tick patches (`Updated`)
//! only the rows whose values changed; a row that shows `Landed` is frozen and
//! then deleted (`Deleted`) `landed_retention` of real time later.

mod aircraft;
mod airports;
mod board;
mod columns;
mod flight;
mod sim;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::time::{Instant, interval};

use crate::effect::{FakerCtx, FakerEffect};
use crate::value_gen::ValueGen;
use board::Board;
use sim::FlightSim;

pub use columns::{FLIGHT_COLUMNS, is_flight_column};
pub use flight::Phase;

/// Settings for a [`FlightsEffect`]. Every field has a default; a YAML
/// `effect: flights` block maps onto it field by field.
#[derive(Clone, Debug)]
pub struct FlightsConfig {
    /// Flights to keep in the air (taxiing to descending), roughly.
    pub fleet: usize,
    /// Flights waiting to depart (scheduled or boarding) at any moment.
    pub board: usize,
    /// Sim seconds per real second.
    pub time_scale: f64,
    /// Real time between updates.
    pub tick: Duration,
    /// Real time a landed row stays before it is deleted.
    pub landed_retention: Duration,
    /// Makes routes, types, schedules and extra columns repeatable. Timing
    /// still follows the real clock.
    pub seed: Option<u64>,
}

impl Default for FlightsConfig {
    fn default() -> Self {
        Self {
            fleet: 40,
            board: 8,
            time_scale: 60.0,
            tick: Duration::from_secs(1),
            landed_retention: Duration::from_secs(60),
            seed: None,
        }
    }
}

impl FlightsConfig {
    /// Largest accepted `fleet`.
    pub const MAX_FLEET: usize = 2000;
    /// Largest accepted `board`.
    pub const MAX_BOARD: usize = 500;

    /// Report settings the sim cannot run with: a `fleet` above
    /// [`MAX_FLEET`](Self::MAX_FLEET), a `board` above
    /// [`MAX_BOARD`](Self::MAX_BOARD), a non-positive or non-finite
    /// `time_scale`, or a zero `tick`.
    pub fn validate(&self) -> Result<(), String> {
        if self.fleet > Self::MAX_FLEET {
            return Err(format!(
                "flights: fleet {} is above the maximum of {}",
                self.fleet,
                Self::MAX_FLEET
            ));
        }
        if self.board > Self::MAX_BOARD {
            return Err(format!(
                "flights: board {} is above the maximum of {}",
                self.board,
                Self::MAX_BOARD
            ));
        }
        if !self.time_scale.is_finite() || self.time_scale <= 0.0 {
            return Err(format!(
                "flights: time_scale {} must be finite and positive",
                self.time_scale
            ));
        }
        if self.tick.is_zero() {
            return Err("flights: tick must be above zero".into());
        }
        Ok(())
    }
}

/// The live flight-simulation effect. See the [module docs](self).
pub struct FlightsEffect {
    cfg: FlightsConfig,
    start: Option<SystemTime>,
    state: Mutex<Option<Board>>,
}

impl FlightsEffect {
    /// An effect running `cfg`. `fleet` and `board` are clamped to their
    /// maximums, an invalid `time_scale` falls back to the default and a zero
    /// `tick` to one second; call [`FlightsConfig::validate`] to report them
    /// instead.
    pub fn new(mut cfg: FlightsConfig) -> Self {
        let defaults = FlightsConfig::default();
        cfg.fleet = cfg.fleet.min(FlightsConfig::MAX_FLEET);
        cfg.board = cfg.board.min(FlightsConfig::MAX_BOARD);
        if !cfg.time_scale.is_finite() || cfg.time_scale <= 0.0 {
            cfg.time_scale = defaults.time_scale;
        }
        if cfg.tick.is_zero() {
            cfg.tick = defaults.tick;
        }
        Self {
            cfg,
            start: None,
            state: Mutex::new(None),
        }
    }

    /// Start the sim clock at `start` instead of the wall-clock time the
    /// table is built — with a seed, the whole table is then reproducible.
    pub fn with_start(mut self, start: SystemTime) -> Self {
        self.start = Some(start);
        self
    }

    /// Advance the sim to `elapsed` real time since the table was seeded and
    /// apply the resulting inserts, patches and deletes to `ctx`. The live
    /// loop calls this every tick; tests and scripted demos can drive it by
    /// hand. A no-op before [`seed`](FakerEffect::seed).
    pub fn advance(&self, ctx: &FakerCtx, elapsed: Duration) {
        if let Some(board) = self.state.lock().unwrap().as_mut() {
            board.advance(ctx, elapsed);
        }
    }
}

#[async_trait]
impl FakerEffect for FlightsEffect {
    fn seed(&self, ctx: &FakerCtx) {
        let start = self.start.unwrap_or_else(SystemTime::now);
        let t0 = start
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64().floor());
        let sim = FlightSim::new(self.cfg.fleet, self.cfg.board, self.cfg.seed, t0);
        let values = match self.cfg.seed {
            Some(seed) => ValueGen::seeded(seed ^ 0xF11_6475).with_now(t0 as i64),
            None => ctx.values().clone(),
        };
        let mut board = Board::new(
            sim,
            t0,
            self.cfg.time_scale,
            self.cfg.landed_retention,
            ctx,
            values,
        );
        board.seed(ctx);
        *self.state.lock().unwrap() = Some(board);
    }

    fn is_live(&self) -> bool {
        true
    }

    async fn run(&self, ctx: Arc<FakerCtx>) {
        let start = Instant::now();
        let mut ticker = interval(self.cfg.tick);
        loop {
            ticker.tick().await;
            self.advance(&ctx, start.elapsed());
        }
    }
}
