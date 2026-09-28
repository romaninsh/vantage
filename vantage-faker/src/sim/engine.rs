//! The engine handle, its builder and the state every sim thread shares.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use vantage_rhai::rhai::{AST, Engine, Map as RhaiMap};
use vantage_rhai::{Host, Limits, Mode, from_json};

use super::clock::{Clock, SimClock, unix_secs};
use super::sched::Sched;
use super::{SimDef, spawn, validate, vocab};
use crate::FakerCtx;

/// Least real time between two error logs of one def.
const ERROR_LOG_EVERY: Duration = Duration::from_secs(60);

/// One def, compiled and bound to its clock.
pub(super) struct Kind {
    pub def: SimDef,
    pub ast: Arc<AST>,
    pub clock: SimClock,
    /// The spawner's args as a Rhai map.
    pub args: RhaiMap,
    errors: Mutex<ErrorLog>,
}

#[derive(Default)]
struct ErrorLog {
    last: Option<Instant>,
    suppressed: u64,
}

impl Kind {
    /// Log a failed sim: the first failure of this def, then at most one
    /// line per [`ERROR_LOG_EVERY`] carrying the count of the ones skipped.
    pub fn report(&self, error: &str) {
        let mut log = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        if log.last.is_some_and(|at| at.elapsed() < ERROR_LOG_EVERY) {
            log.suppressed += 1;
            return;
        }
        let suppressed = std::mem::take(&mut log.suppressed);
        log.last = Some(Instant::now());
        tracing::error!(sim = %self.def.name, suppressed, %error, "faker sim failed; it ended");
    }
}

/// Everything the sim threads share.
pub(super) struct Inner {
    pub engine: Arc<Engine>,
    pub kinds: Vec<Kind>,
    pub by_name: HashMap<String, usize>,
    pub tables: HashMap<String, Weak<FakerCtx>>,
    pub sched: Sched,
    pub seed: Option<u64>,
    /// Wall time the engine started at; every sim clock meets it there.
    pub origin: f64,
    pub handles: Mutex<Vec<JoinHandle<()>>>,
}

impl Inner {
    /// The live store handle of `table`, if the table still exists.
    pub fn table(&self, table: &str) -> Option<Arc<FakerCtx>> {
        self.tables.get(table)?.upgrade()
    }

    /// Whether any table is still alive.
    pub fn tables_alive(&self) -> bool {
        self.tables.values().any(|t| t.strong_count() > 0)
    }

    fn set_quiet(&self, quiet: bool) {
        for ctx in self.tables.values().filter_map(Weak::upgrade) {
            ctx.set_quiet(quiet);
        }
    }
}

/// Configures and starts a [`SimEngine`].
#[derive(Default)]
pub struct SimEngineBuilder {
    tables: Vec<(String, Weak<FakerCtx>)>,
    defs: Vec<SimDef>,
    seed: Option<u64>,
    manual: Option<SystemTime>,
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

        let (clock, origin) = match self.manual {
            Some(start) => (Clock::Manual(unix_secs(start)), unix_secs(start)),
            None => (Clock::System, unix_secs(SystemTime::now())),
        };
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
        });

        let mut plan = spawn::Plan::new(&inner);
        inner.set_quiet(true);
        spawn::warm(&inner, &mut plan);
        inner.set_quiet(false);
        let driver = spawn::start_driver(inner.clone(), plan);
        Ok(SimEngine {
            inner,
            driver: Mutex::new(driver),
        })
    }
}

/// Runs the sims of one datasource. Dropping it stops every sim and joins
/// their threads.
pub struct SimEngine {
    inner: Arc<Inner>,
    driver: Mutex<Option<JoinHandle<()>>>,
}

impl SimEngine {
    pub fn builder() -> SimEngineBuilder {
        SimEngineBuilder::default()
    }

    /// Sims alive now, over all defs.
    pub fn live(&self) -> usize {
        self.inner.sched.lock().live_total
    }

    /// Sims of def `name` alive now.
    pub fn live_of(&self, name: &str) -> usize {
        let Some(&kind) = self.inner.by_name.get(name) else {
            return 0;
        };
        self.inner.sched.lock().live[kind]
    }

    /// Sim threads not yet ended.
    pub fn threads(&self) -> usize {
        self.inner.sched.lock().threads
    }

    /// Wait until every sim is asleep or ended and no sleeper is due.
    pub fn settle(&self) {
        self.inner.sched.settle();
    }

    /// Move a [manual clock](SimEngineBuilder::manual_clock) forward by `by`
    /// and wake the sims now due. Returns `false` (and does nothing) on the
    /// system clock.
    pub fn advance(&self, by: Duration) -> bool {
        self.inner.sched.advance(by.as_secs_f64())
    }

    #[cfg(test)]
    pub(super) fn inner_for_tests(&self) -> Arc<Inner> {
        self.inner.clone()
    }

    /// Stop every sim and wait for their threads to end. Idempotent.
    pub fn stop(&self) {
        self.inner.sched.stop();
        let driver = self.driver.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(driver) = driver {
            let _ = driver.join();
        }
        self.inner.sched.wait_no_threads();
        let handles =
            std::mem::take(&mut *self.inner.handles.lock().unwrap_or_else(|e| e.into_inner()));
        for handle in handles {
            let _ = handle.join();
        }
    }
}

impl Drop for SimEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
