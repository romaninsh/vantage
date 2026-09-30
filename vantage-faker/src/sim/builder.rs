//! Configuring and starting a [`SimEngine`].

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use vantage_core::{Result, error};
use vantage_rhai::rhai::Map as RhaiMap;
use vantage_rhai::{Host, Limits, Mode, from_json};

use super::clock::{Clock, SimClock, unix_secs};
use super::engine::SimEngine;
use super::kind::{Inner, Kind, OnSimError};
use super::sched::Sched;
use super::{SimDef, spawn, validate, vocab};
use crate::FakerColumn;
use vantage_memory::MemoryStore;

/// Configures and starts a [`SimEngine`].
#[derive(Default)]
pub struct SimEngineBuilder {
    store: MemoryStore,
    defs: Vec<SimDef>,
    seed: Option<u64>,
    manual: Option<SystemTime>,
    warm_progress: Option<Box<spawn::Progress>>,
    columns: HashMap<String, Vec<FakerColumn>>,
    on_error: Option<Arc<OnSimError>>,
}

impl SimEngineBuilder {
    /// Run the sims against `store`'s tables. The engine keeps the store
    /// alive; without this call it gets an empty store of its own.
    pub fn store(mut self, store: &MemoryStore) -> Self {
        self.store = store.clone();
        self
    }

    /// Add a sim def.
    pub fn sim(mut self, def: SimDef) -> Self {
        self.defs.push(def);
        self
    }

    /// Declare `table`'s columns, so the `row()` verb can generate a value
    /// for each. A table with no call here has no declared columns, and
    /// `row()` on it always returns an empty map.
    pub fn columns(mut self, table: impl Into<String>, columns: Vec<FakerColumn>) -> Self {
        self.columns.insert(table.into(), columns);
        self
    }

    /// Make every sim's random draws repeatable. Thread timing can still
    /// reorder spawns made by scripts.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Run on a clock that starts at `start` and only moves on
    /// [`SimEngine::advance`] — for tests and scripted demos.
    pub fn manual_clock(mut self, start: SystemTime) -> Self {
        self.manual = Some(start);
        self
    }

    /// Call `f` from [`start`](Self::start) as the warm start runs, with the
    /// fraction done (`0 < p ≤ 1`) after each of its windows. Never called
    /// when no def has a warm span.
    pub fn on_warm_progress(mut self, f: impl Fn(f32) + Send + Sync + 'static) -> Self {
        self.warm_progress = Some(Box::new(f));
        self
    }

    /// Call `f(def_name, error)` from a sim's own thread every time a sim
    /// of that def ends in error, live or during the warm start. Every
    /// error calls `f`, unlike the tracing log next to it, which is
    /// rate-limited per def.
    pub fn on_sim_error(mut self, f: impl Fn(&str, &str) + Send + Sync + 'static) -> Self {
        self.on_error = Some(Arc::new(f));
        self
    }

    /// Validate, compile, run the warm start to completion and go live.
    ///
    /// Blocks until the warm start is done. The warm start quiets every
    /// store table and unquiets them all when it ends, so a table the
    /// caller had quieted comes out unquieted. If the warm start unwinds (a
    /// panicking [`on_warm_progress`](Self::on_warm_progress) callback), its
    /// sims are stopped and joined and the tables unquieted before the
    /// panic propagates.
    ///
    /// Fails on the first invalid def (see [`SimDef::validate`]), a
    /// duplicate name, a default table the store does not have, or `max`es
    /// adding up to more than [`MAX_LIVE`](super::MAX_LIVE).
    pub fn start(self) -> Result<SimEngine> {
        let names: HashSet<String> = self.store.table_names().into_iter().collect();
        validate::validate_all(&self.defs, &names).map_err(|e| error!(e))?;

        let stop = Arc::new(AtomicBool::new(false));
        let progress_stop = stop.clone();
        let host = Host::builder(Limits::background())
            .vocab_fn(move |engine| vocab::register(engine, progress_stop))
            .build();

        let clock = match self.manual {
            Some(start) => Clock::Manual(unix_secs(start)),
            None => Clock::system(),
        };
        let origin = clock.now();
        let mut kinds = Vec::with_capacity(self.defs.len());
        for def in self.defs {
            let ast = host
                .ast_uncached(Mode::Script, &def.script)
                .map_err(|e| error!(format!("sim {}: script does not compile: {e}", def.name)))?;
            let args = from_json(&serde_json::Value::Object(def.spawn.args.clone()))
                .try_cast::<RhaiMap>()
                .unwrap_or_default();
            kinds.push(Kind {
                clock: SimClock {
                    origin,
                    scale: def.clock,
                },
                ast,
                args,
                def,
                errors: Mutex::default(),
                row_state: Mutex::default(),
            });
        }
        let by_name = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| (k.def.name.clone(), i))
            .collect();
        let inner = Arc::new(Inner {
            engine: host.engine().clone(),
            sched: Sched::new(clock, origin, kinds.len(), stop),
            kinds,
            by_name,
            store: self.store,
            tables: Default::default(),
            columns: self.columns,
            seed: self.seed,
            origin,
            handles: Mutex::default(),
            counters: Default::default(),
            on_error: self.on_error,
        });

        let mut plan = spawn::Plan::new(&inner);
        spawn::warm(&inner, &mut plan, self.warm_progress.as_deref());
        let driver = spawn::start_driver(inner.clone(), plan);
        Ok(SimEngine::new(inner, driver))
    }
}
