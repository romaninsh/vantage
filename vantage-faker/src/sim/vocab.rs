//! The verbs a sim script can call.
//!
//! **Data** — `table()` and `table(name)` return a
//! [`Handle`](vantage_vista::Handle) over the engine's store; `table()`
//! defaults to the def's own table. Either form narrows (`where`, `sort`,
//! `search`, `limit`) and resolves (`insert`, `upsert`, `patch`, `delete`,
//! `get`, `ids`, `count`, `list`, `first`) exactly like vantage-vista's
//! `DataVocab` elsewhere; a missing table is a script error. Writes
//! broadcast the store's `MemoryChange`s (during the warm start each
//! written table sends one `Reset` instead, at its end). Row ids are
//! strings: the verbs return them as strings and take them as strings (a
//! number is not an id; an `insert` map's id column is stored under its
//! string form).
//! - `table().fake_row()` / `table(name).fake_row() -> map` — a generated
//!   value for each of the table's declared columns
//!   ([`SimEngineBuilder::columns`](super::SimEngineBuilder::columns)),
//!   skipping the id column; a table with none declared gives an empty map.
//!   `walk` and even-spread `date` columns advance one step per call, kept
//!   per def and table; a `tree` column is not meaningful here — it plans a
//!   whole table's parent links from a row count `fake_row()` never has, so
//!   each call just walks further into that fixed plan and can return a
//!   parent id no row `fake_row()` ever produced.
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

mod counted;
mod random;
mod row;
mod tables;
mod time;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use vantage_memory::MemoryStore;
use vantage_memory::vista::Catalog;
use vantage_rhai::Vocab;
use vantage_rhai::rhai::{Dynamic, Engine};
use vantage_vista::DataVocab;

use super::current::{self, VerbResult};
use crate::FakerColumn;

/// Register every verb and the progress hook on `engine`. The operation
/// budget is per stretch between sleeps, enforced by the hook, so the
/// engine-wide ceiling is lifted.
pub(super) fn register(
    engine: &mut Engine,
    stop: Arc<AtomicBool>,
    store: MemoryStore,
    catalog: Catalog,
    columns: Arc<HashMap<String, Vec<FakerColumn>>>,
    write_counter: Arc<AtomicU64>,
) {
    engine.set_max_operations(0);
    engine.set_max_call_levels(super::SIM_CALL_LEVELS);
    engine.set_max_expr_depths(super::SIM_EXPR_DEPTHS.0, super::SIM_EXPR_DEPTHS.1);
    engine.on_progress(move |ops| current::progress(ops, &stop));
    engine.on_print(|s| tracing::info!(target: "faker_sim", "{s}"));
    engine.on_debug(|s, _, _| tracing::debug!(target: "faker_sim", "{s}"));

    DataVocab::read_write(Some(tables::memory_resolver(
        store,
        catalog,
        columns,
        write_counter,
    )))
    .register(engine);
    tables::register(engine);
    random::register(engine);
    row::register(engine);
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
