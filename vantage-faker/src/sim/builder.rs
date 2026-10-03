//! Configuring and starting a [`SimEngine`].

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;

use vantage_rhai::rhai::Map as RhaiMap;
use vantage_rhai::{Host, Limits, Mode, from_json};

use super::clock::{Clock, SimClock, unix_secs};
use super::engine::SimEngine;
use super::kind::{Inner, Kind};
use super::sched::Sched;
use super::{SimDef, spawn, validate, vocab};
use crate::FakerCtx;

/// Configures and starts a [`SimEngine`].
#[derive(Default)]
pub struct SimEngineBuilder {
    tables: Vec<(String, Weak<FakerCtx>)>,
    defs: Vec<SimDef>,
    seed: Option<u64>,
    manual: Option<SystemTime>,
    warm_progress: Option<Box<spawn::Progress>>,
}

impl SimEngineBuilder {
    /// Make `ctx` (a [`FakerTable::ctx`](crate::FakerTable::ctx)) writable
    /// as `name`. The engine holds it weakly.
    pub fn table(mut self, name: impl Into<String>, ctx: &Arc<FakerCtx>) -> Self {
        self.tables.push((name.into(), Arc::downgrade(ctx)));
        self
    }

    /// Add a sim def.
    pub fn sim(mut self, def: SimDef) -> Self {
        self.defs.push(def);
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

    /// Validate, compile, run the warm start to completion and go live.
    ///
    /// Blocks until the warm start is done. Fails on the first invalid def
    /// (see [`SimDef::validate`]), a duplicate name, a default table that
    /// was not added, or `max`es adding up to more than
    /// [`MAX_LIVE`](super::MAX_LIVE).
    pub fn start(self) -> Result<SimEngine, String> {
        let names: HashSet<&str> = self.tables.iter().map(|(n, _)| n.as_str()).collect();
        validate::validate_all(&self.defs, &names)?;

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
                .map_err(|e| format!("sim {}: script does not compile: {e}", def.name))?;
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
            tables: self.tables.into_iter().collect(),
            seed: self.seed,
            origin,
            handles: Mutex::default(),
            counters: Default::default(),
        });

        let mut plan = spawn::Plan::new(&inner);
        inner.set_quiet(true);
        spawn::warm(&inner, &mut plan, self.warm_progress.as_deref());
        inner.set_quiet(false);
        let driver = spawn::start_driver(inner.clone(), plan);
        Ok(SimEngine::new(inner, driver))
    }
}
