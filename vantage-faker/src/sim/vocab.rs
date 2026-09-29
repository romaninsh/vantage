//! The verbs a sim script can call.
//!
//! **Data** — `table` is optional everywhere and defaults to the def's table;
//! it can name any table of the engine's store; a missing table is a script
//! error. Writes broadcast the store's `MemoryChange`s (during the warm start
//! each written table sends one `Reset` instead, at its end). Row ids are
//! strings: the verbs return them as strings and take them as strings (a
//! number is not an id; an `insert` map's id column is stored under its
//! string form).
//! - `insert(table?, #{…}) -> id` — a new row, stored as the map gives it.
//!   The map's id column, if set, is the row id (an existing row with it is
//!   a script error); otherwise an id is assigned.
//! - `upsert(table?, id, #{…})` — insert or replace row `id`.
//! - `patch(table?, id, #{…})`, `set(table?, id, field, value)` — change a
//!   row; a missing row is ignored.
//! - `delete(table?, id)`, `get(table?, id) -> map or ()`, `ids(table?)`,
//!   `count(table?)`.
//! - `find(table?, #{col: value, …}) -> [id]` — ids of the rows equal to
//!   every entry, in insertion order; an empty map gives every id.
//!
//! **Spawn** — `spawn_sim(name, #{args}?) -> bool` starts a sim of def
//! `name` at this sim's current time; `false` when the def is at its `max`
//! or the engine at its cap. (`spawn` is a reserved word in Rhai.)
//!
//! **Random** — `pick(array)`, `pick_weighted(array, weights)`,
//! `rand_int(lo, hi)` (inclusive), `rand_float(lo, hi)`, `chance(p)`,
//! `pattern("BA####")` (`#` digit, `?` letter, `*` either),
//! `sentence(min, max)` (words), `fake(kind)` (a value as a column named
//! `kind` gets: `first_name`, `last_name`, `name`, `city`, `country`,
//! `company`, `email`, `phone`, `street`, …) and `date_between(from, to)`
//! (dates as column generators take them: RFC 3339, `now`, `-2d`, …,
//! relative to the sim clock; returns RFC 3339).
//!
//! **Time** — durations are sim seconds (floats): `seconds(n)`,
//! `minutes(n)`, `hours(n)`, `days(n)`. `sleep(d)` and `wait_until(t)` (sim
//! unix seconds, or a date string as above) block the sim. `now()` is the
//! sim clock and `wall_now()` the wall clock, both RFC 3339; `wall_in(d)` is
//! the wall-clock instant the sim clock reaches `now + d`, so a row can show
//! honest real-time ETAs. `now_secs()` is the sim clock as unix seconds,
//! `elapsed()` the sim seconds since this sim started and `clock()` the
//! def's speed. `done()` ends the sim early; its rows stay.
//!
//! **Geo** — `great_circle(lat1, lon1, lat2, lon2) -> km`,
//! `interpolate(lat1, lon1, lat2, lon2, frac) -> #{lat, lon}` and
//! `bearing(lat1, lon1, lat2, lon2) -> degrees`.
//!
//! **Misc** — `sim_id()`, `sim_name()`; `print` / `debug` go to the log.
//! The spawner's (or `spawn` caller's) args are the variable `args`.

pub(crate) mod convert;
mod data;
mod random;
mod time;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use vantage_rhai::rhai::{Dynamic, Engine};

use super::current::{self, VerbResult};

/// Register every verb and the progress hook on `engine`. The operation
/// budget is per stretch between sleeps, enforced by the hook, so the
/// engine-wide ceiling is lifted.
pub(super) fn register(engine: &mut Engine, stop: Arc<AtomicBool>) {
    engine.set_max_operations(0);
    engine.set_max_call_levels(super::SIM_CALL_LEVELS);
    engine.set_max_expr_depths(super::SIM_EXPR_DEPTHS.0, super::SIM_EXPR_DEPTHS.1);
    engine.on_progress(move |ops| current::progress(ops, &stop));
    engine.on_print(|s| tracing::info!(target: "faker_sim", "{s}"));
    engine.on_debug(|s, _, _| tracing::debug!(target: "faker_sim", "{s}"));
    data::register(engine);
    random::register(engine);
    time::register(engine);
}

/// A script number (int or float) as `f64`.
fn num(v: &Dynamic) -> VerbResult<f64> {
    if let Ok(f) = v.as_float() {
        Ok(f)
    } else if let Ok(i) = v.as_int() {
        Ok(i as f64)
    } else {
        Err(format!("expected a number, got {}", v.type_name()).into())
    }
}
