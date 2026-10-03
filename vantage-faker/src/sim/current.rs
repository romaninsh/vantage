//! The running sim on this thread: its clock, rng and budget, and the loop
//! body of a sim thread.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fake::rand::SeedableRng as _;
use fake::rand::rngs::StdRng;
use vantage_rhai::rhai::{Dynamic, EvalAltResult, Map as RhaiMap, Position, Scope};

use super::DEFAULT_OPS;
use super::kind::{Inner, Kind};
use super::stats::Counters;

/// A verb's result.
pub(super) type VerbResult<T> = Result<T, Box<EvalAltResult>>;

/// Sleeps in a row that may leave the sim's clock where it is before the
/// sim is ended as a runaway loop.
pub(super) const MAX_STILL_SLEEPS: u32 = 1000;

pub(super) struct Current {
    pub inner: Arc<Inner>,
    pub kind: usize,
    pub id: u64,
    /// This sim's clock, sim unix seconds. Never ahead of the live sim
    /// clock; behind it only during a warm start.
    pub vt: f64,
    /// Sim time the sim started at.
    pub started: f64,
    pub rng: StdRng,
    /// Set by `done()`: the script is ending on purpose.
    pub done: bool,
    /// Rhai operations allowed between two sleeps: the def's `ops`.
    ops_budget: u64,
    ops_base: u64,
    last_ops: u64,
    /// Sleeps in a row that did not move `vt`.
    still_sleeps: u32,
}

thread_local! {
    static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
}

/// Run `f` against the sim on this thread.
pub(super) fn with<R>(f: impl FnOnce(&mut Current) -> VerbResult<R>) -> VerbResult<R> {
    CURRENT.with_borrow_mut(|c| match c {
        Some(c) => f(c),
        None => Err("sim verbs only work inside a running sim".into()),
    })
}

/// A non-catchable error that ends the script.
pub(super) fn terminate(why: &str) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorTerminated(
        Dynamic::from(why.to_string()),
        Position::NONE,
    ))
}

/// The engine's `on_progress` hook: ends the script when the engine stops,
/// after `done()`, or when it spends too long without sleeping.
pub(super) fn progress(ops: u64, stop: &AtomicBool) -> Option<Dynamic> {
    if stop.load(Ordering::Relaxed) {
        return Some("sim engine stopped".into());
    }
    CURRENT.with_borrow_mut(|c| {
        let c = c.as_mut()?;
        c.last_ops = ops;
        if c.done {
            return Some("done".into());
        }
        (ops.saturating_sub(c.ops_base) > c.ops_budget)
            .then(|| "sim ran too many operations without sleeping".into())
    })
}

impl Current {
    pub fn kind(&self) -> &Kind {
        &self.inner.kinds[self.kind]
    }

    /// Block until this sim's clock reaches `target` (sim seconds).
    ///
    /// A target at or before the sim's clock returns at once and leaves the
    /// clock and the operation budget as they are; [`MAX_STILL_SLEEPS`] of
    /// those in a row end the sim.
    pub fn sleep_until(&mut self, target: f64) -> VerbResult<()> {
        if !target.is_finite() {
            return Err(format!("sleep target {target} is not a finite time").into());
        }
        if self.inner.sched.is_stopped() {
            return Err(terminate("sim engine stopped"));
        }
        if target <= self.vt {
            self.still_sleeps += 1;
            if self.still_sleeps >= MAX_STILL_SLEEPS {
                return Err(format!(
                    "sim slept {MAX_STILL_SLEEPS} times in a row without its clock moving"
                )
                .into());
            }
            return Ok(());
        }
        self.still_sleeps = 0;
        let wall = self.kind().clock.wall(target);
        if self.inner.sched.park(self.id, wall).is_err() {
            return Err(terminate("sim engine stopped"));
        }
        self.vt = target;
        self.ops_base = self.last_ops;
        Ok(())
    }
}

/// Undo a sim's slot in the counters once its thread is gone (or never
/// started).
pub(super) fn release(inner: &Inner, kind: usize) {
    let mut st = inner.sched.lock();
    st.live[kind] -= 1;
    st.live_total -= 1;
    inner.sched.thread_ended(&mut st);
    inner.sched.ended(&mut st);
}

/// Releases the sim's counters even if a verb panics.
struct Release(Arc<Inner>, usize);

impl Drop for Release {
    fn drop(&mut self) {
        CURRENT.with_borrow_mut(|c| c.take());
        // A panicking verb unwinds straight through run_sim, past its own
        // ended/errored bump, so count it here instead.
        if std::thread::panicking() {
            Counters::bump(&self.0.counters.errored);
        }
        release(&self.0, self.1);
    }
}

/// Body of a sim thread: run the def's script once with `args` in scope.
pub(super) fn run_sim(inner: Arc<Inner>, kind: usize, id: u64, vt: f64, args: RhaiMap) {
    let _release = Release(inner.clone(), kind);
    let rng = match inner.seed {
        Some(seed) => {
            let salt = ((kind as u64) << 48) ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            StdRng::seed_from_u64(seed ^ salt)
        }
        None => crate::value_gen::entropy_rng(),
    };
    CURRENT.set(Some(Current {
        inner: inner.clone(),
        kind,
        id,
        vt,
        started: vt,
        rng,
        done: false,
        ops_budget: inner.kinds[kind].def.ops.unwrap_or(DEFAULT_OPS),
        ops_base: 0,
        last_ops: 0,
        still_sleeps: 0,
    }));
    let mut scope = Scope::new();
    scope.push("args", args);
    let k = &inner.kinds[kind];
    let result = inner.engine.run_ast_with_scope(&mut scope, &k.ast);
    let done = CURRENT.with_borrow(|c| c.as_ref().is_some_and(|c| c.done));
    let failed = result.is_err() && !done && !inner.sched.is_stopped();
    if failed && let Err(e) = &result {
        let msg = e.to_string();
        if let Some(on_error) = &inner.on_error {
            on_error(&k.def.name, &msg);
        }
        k.report(&msg);
    }
    Counters::bump(if failed {
        &inner.counters.errored
    } else {
        &inner.counters.ended
    });
}
