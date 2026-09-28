//! Sims — Rhai scripts that live inside a faker datasource.
//!
//! A [`SimDef`] names a script, the table it writes to by default, a
//! [`Spawn`]er that decides how many copies run, a clock speed and an
//! optional warm start. A [`SimEngine`] runs every def of one datasource
//! against that datasource's tables.
//!
//! **Linear scripts.** Each live sim is one run of its script, top to bottom,
//! on its own small thread. The script's local variables are its state;
//! `sleep(d)` and `wait_until(t)` block that sim until its clock reaches the
//! target, and the sim ends when the script does (or calls `done()`):
//!
//! ```rhai
//! let id = insert(#{ status: "Booked" });
//! sleep(minutes(20));
//! patch(id, #{ status: "Picked up" });
//! while get(id).progress < 100.0 { patch(id, #{ progress: get(id).progress + 5.0 }); sleep(seconds(30)); }
//! sleep(minutes(1));
//! delete(id);
//! ```
//!
//! **Clocks.** Each def runs on its own sim clock: `sim = start + elapsed ×
//! clock`, where `start` is the wall-clock time the engine started. Every
//! duration a script handles is sim time. A sleeping sim costs a parked
//! thread and nothing else.
//!
//! **Warm start.** A def with `warm: Some(d)` begins `d` of sim time in the
//! past: its burst and its rate spawns within that window run instantly, in
//! virtual time, before [`SimEngineBuilder::start`] returns, so tables open
//! mid-life. Warm writes do not broadcast, like an effect's `seed`.
//!
//! **Spawning.** `burst` sims start at the beginning (of the warm window, or
//! of the live run), then `rate_per_min` more per sim minute, never more than
//! `max` at once. Scripts can `spawn_sim(name, #{args})` any def, subject to its
//! `max`. At most [`MAX_LIVE`] sims run per engine.
//!
//! See the `vocab` module docs for the verbs scripts can call, and
//! `examples/sims/*.rhai` for complete scripts.

mod clock;
mod current;
mod engine;
mod sched;
mod spawn;
#[cfg(test)]
mod tests;
mod validate;
mod vocab;

use std::time::Duration;

pub use engine::{SimEngine, SimEngineBuilder};

/// Most sims one engine runs at once; also the ceiling on the sum of every
/// def's `max`.
pub const MAX_LIVE: usize = 1000;

/// Stack size of a sim thread. Unoptimised Rhai needs about ten times the
/// stack per call level, so debug builds get more.
pub const SIM_STACK_BYTES: usize = if cfg!(debug_assertions) {
    4 * 1024 * 1024
} else {
    512 * 1024
};

/// Deepest script function nesting a sim may reach; with the expression
/// depth limits it keeps a runaway recursion inside [`SIM_STACK_BYTES`].
pub(crate) const SIM_CALL_LEVELS: usize = 32;
/// Deepest expression nesting in a sim script, at top level and inside
/// functions.
pub(crate) const SIM_EXPR_DEPTHS: (usize, usize) = (64, 32);

/// How many sims of a def run, and when they start.
#[derive(Clone, Debug)]
pub struct Spawn {
    /// Sims started at once when the def begins.
    pub burst: usize,
    /// Sims started per sim minute after the burst. Zero for none.
    pub rate_per_min: f64,
    /// Most sims of this def alive at once. Spawns above it are skipped.
    pub max: usize,
    /// Handed to every spawner-started sim as `args`.
    pub args: serde_json::Map<String, serde_json::Value>,
}

impl Default for Spawn {
    /// One sim at the start, none after.
    fn default() -> Self {
        Self {
            burst: 1,
            rate_per_min: 0.0,
            max: 1,
            args: serde_json::Map::new(),
        }
    }
}

/// One kind of sim: a script and how copies of it are spawned.
#[derive(Clone, Debug)]
pub struct SimDef {
    /// Name other scripts `spawn` it by.
    pub name: String,
    /// Table the data verbs write to when a script names none.
    pub table: String,
    /// The Rhai script one sim runs, top to bottom.
    pub script: String,
    pub spawn: Spawn,
    /// Sim seconds per real second. Default 1.
    pub clock: f64,
    /// Sim time to run instantly before going live. Default none.
    pub warm: Option<Duration>,
}

impl SimDef {
    /// A def with the default [`Spawn`], a real-time clock and no warm start.
    pub fn new(
        name: impl Into<String>,
        table: impl Into<String>,
        script: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            script: script.into(),
            spawn: Spawn::default(),
            clock: 1.0,
            warm: None,
        }
    }

    /// Replace the spawner.
    pub fn with_spawn(mut self, burst: usize, rate_per_min: f64, max: usize) -> Self {
        self.spawn.burst = burst;
        self.spawn.rate_per_min = rate_per_min;
        self.spawn.max = max;
        self
    }

    /// Set the spawner's `args`.
    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        if let serde_json::Value::Object(map) = args {
            self.spawn.args = map;
        }
        self
    }

    /// Set the clock speed.
    pub fn with_clock(mut self, clock: f64) -> Self {
        self.clock = clock;
        self
    }

    /// Start `warm` of sim time in the past.
    pub fn with_warm(mut self, warm: Duration) -> Self {
        self.warm = Some(warm);
        self
    }
}
