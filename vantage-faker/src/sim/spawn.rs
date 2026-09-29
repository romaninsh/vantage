//! Starting sims: one thread per sim, the spawner schedule, the warm
//! catch-up and the live driver thread.

use std::sync::Arc;
use std::thread::JoinHandle;

use vantage_rhai::rhai::Map as RhaiMap;

use super::kind::Inner;
use super::stats::Counters;
use super::{MAX_LIVE, SIM_STACK_BYTES, current};

/// Windows the warm start is cut into. Sims run in parallel within a window
/// and wait for each other at its end, so cross-sim ordering and `max` are
/// exact to one window (1/256 of the warm span).
const WARM_WINDOWS: u32 = 256;

/// Warm-start progress callback; see
/// [`SimEngineBuilder::on_warm_progress`](super::SimEngineBuilder::on_warm_progress).
pub(super) type Progress = dyn Fn(f32) + Send + Sync;

/// Park id of the driver; sim ids start at 1.
const DRIVER_ID: u64 = 0;

/// The next spawner event of one def.
#[derive(Clone, Copy, Debug)]
struct Due {
    /// Wall time of the event; infinite when there is none.
    at: f64,
    /// Whether the event is the burst (else one rate spawn).
    burst: bool,
    /// Wall seconds between rate spawns; infinite for no rate.
    every: f64,
}

/// Every def's spawner schedule.
pub(super) struct Plan(Vec<Due>);

impl Plan {
    /// Bursts at the start of each def's warm window (or at the engine
    /// start), rate spawns after.
    pub fn new(inner: &Inner) -> Self {
        Plan(
            inner
                .kinds
                .iter()
                .map(|k| {
                    let warm = k.def.warm.map_or(0.0, |w| w.as_secs_f64());
                    let rate = k.def.spawn.rate_per_min;
                    Due {
                        at: inner.origin - warm / k.clock.scale,
                        burst: true,
                        every: if rate > 0.0 {
                            60.0 / rate / k.clock.scale
                        } else {
                            f64::INFINITY
                        },
                    }
                })
                .collect(),
        )
    }

    fn earliest(&self) -> f64 {
        self.0.iter().map(|d| d.at).fold(f64::INFINITY, f64::min)
    }

    /// Run every event before `until` (or at it, when `inclusive`).
    fn run_due(&mut self, inner: &Arc<Inner>, until: f64, inclusive: bool) {
        for (kind, due) in self.0.iter_mut().enumerate() {
            while due.at < until || (inclusive && due.at == until) {
                let k = &inner.kinds[kind];
                let n = if due.burst { k.def.spawn.burst } else { 1 };
                for _ in 0..n {
                    spawn_sim(inner, kind, k.clock.sim(due.at), k.args.clone());
                }
                due.at += due.every;
                due.burst = false;
                if inner.sched.is_stopped() {
                    return;
                }
            }
        }
    }
}

/// Start sim `kind` at sim time `vt` with `args`, unless the engine stopped,
/// [`MAX_LIVE`] sims run or the def is at its `max`. Returns whether it
/// started.
pub(super) fn spawn_sim(inner: &Arc<Inner>, kind: usize, vt: f64, args: RhaiMap) -> bool {
    let id = {
        let mut st = inner.sched.lock();
        if st.stopped {
            return false;
        }
        let name = &inner.kinds[kind].def.name;
        if st.live[kind] >= inner.kinds[kind].def.spawn.max {
            tracing::debug!(sim = %name, "faker sim spawn skipped: def at its max");
            return false;
        }
        if st.live_total >= MAX_LIVE {
            tracing::debug!(sim = %name, "faker sim spawn skipped: engine at {MAX_LIVE} live sims");
            return false;
        }
        st.live_total += 1;
        st.live[kind] += 1;
        st.running += 1;
        st.threads += 1;
        st.next_id += 1;
        st.next_id - 1
    };
    let for_thread = inner.clone();
    let spawned = std::thread::Builder::new()
        .name(format!("faker-sim-{}", inner.kinds[kind].def.name))
        .stack_size(SIM_STACK_BYTES)
        .spawn(move || current::run_sim(for_thread, kind, id, vt, args));
    match spawned {
        Ok(handle) => {
            Counters::bump(&inner.counters.spawned);
            let mut handles = inner.handles.lock().unwrap_or_else(|e| e.into_inner());
            handles.retain(|h| !h.is_finished());
            handles.push(handle);
            true
        }
        Err(e) => {
            current::release(inner, kind);
            tracing::error!(sim = %inner.kinds[kind].def.name, error = %e, "could not start a faker sim thread");
            false
        }
    }
}

/// Run the warm start: every spawner event and sim step before the engine
/// start, window by window, as fast as the sims compute, reporting the
/// fraction done to `progress` after each window.
pub(super) fn warm(inner: &Arc<Inner>, plan: &mut Plan, progress: Option<&Progress>) {
    let begin = plan.earliest();
    let end = inner.origin;
    if begin >= end {
        return;
    }
    inner.sched.set_barrier(begin);
    for w in 1..=WARM_WINDOWS {
        let until = if w == WARM_WINDOWS {
            end
        } else {
            begin + (end - begin) * f64::from(w) / f64::from(WARM_WINDOWS)
        };
        plan.run_due(inner, until, false);
        inner.sched.set_barrier(until);
        inner.sched.settle();
        if let Some(progress) = progress {
            progress(w as f32 / WARM_WINDOWS as f32);
        }
        if inner.sched.is_stopped() {
            break;
        }
    }
    inner.sched.set_barrier(f64::INFINITY);
}

/// Start the thread that runs spawner events as the clock reaches them.
pub(super) fn start_driver(inner: Arc<Inner>, mut plan: Plan) -> Option<JoinHandle<()>> {
    inner.sched.lock().running += 1;
    let for_thread = inner.clone();
    let spawned = std::thread::Builder::new()
        .name("faker-sim-driver".into())
        .stack_size(SIM_STACK_BYTES)
        .spawn(move || {
            let inner = for_thread;
            loop {
                let now = inner.sched.now();
                plan.run_due(&inner, now, true);
                if inner.sched.park(DRIVER_ID, plan.earliest()).is_err() {
                    break;
                }
            }
            let mut st = inner.sched.lock();
            inner.sched.ended(&mut st);
        });
    match spawned {
        Ok(handle) => Some(handle),
        Err(e) => {
            let mut st = inner.sched.lock();
            inner.sched.ended(&mut st);
            tracing::error!(error = %e, "could not start the faker sim driver; no live spawns");
            None
        }
    }
}
