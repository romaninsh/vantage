//! Shared scheduling state: who is running, who sleeps until when, the warm
//! barrier and the stop flag. Sleepers block on one condvar and are woken by
//! a stop, a raised barrier, a manual clock advance or their own timeout.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::clock::Clock;

/// Longest single condvar wait; a sleeper re-checks the clock at least this
/// often.
const MAX_WAIT: Duration = Duration::from_secs(3600);
/// How often `settle` re-checks for sleepers that became due on the system
/// clock, which wakes nobody.
const SETTLE_POLL: Duration = Duration::from_millis(20);

/// The engine is stopping; the sleeper must end its script.
#[derive(Debug)]
pub(super) struct Stopped;

pub(super) struct State {
    pub clock: Clock,
    /// Wall time up to which warm sims may run; infinite once live.
    pub barrier: f64,
    /// Sims and the driver currently executing (not parked, not ended).
    pub running: usize,
    /// Live sims per def, by def index.
    pub live: Vec<usize>,
    pub live_total: usize,
    /// Sim threads not yet ended.
    pub threads: usize,
    pub stopped: bool,
    pub next_id: u64,
    /// Parked sims (and the driver) by id, with the wall time they wait for.
    parked: HashMap<u64, f64>,
}

impl State {
    /// Whether any parked sleeper is due to run now.
    fn any_due(&self) -> bool {
        let limit = self.barrier.min(self.clock.now());
        self.parked.values().any(|t| *t <= limit)
    }
}

pub(super) struct Sched {
    state: Mutex<State>,
    /// Wakes sleepers and the driver.
    wake: Condvar,
    /// Wakes `settle` and `stop` when a sim parks or ends.
    idle: Condvar,
    stop: Arc<AtomicBool>,
    /// `State::barrier` as f64 bits, for the lock-free fast path of `park`.
    barrier: AtomicU64,
    /// Wall time the engine started at; the warm barrier never passes it.
    origin: f64,
}

impl Sched {
    pub fn new(clock: Clock, origin: f64, kinds: usize, stop: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(State {
                clock,
                barrier: f64::INFINITY,
                running: 0,
                live: vec![0; kinds],
                live_total: 0,
                threads: 0,
                stopped: false,
                next_id: 1,
                parked: HashMap::new(),
            }),
            wake: Condvar::new(),
            idle: Condvar::new(),
            stop,
            barrier: AtomicU64::new(f64::INFINITY.to_bits()),
            origin,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn now(&self) -> f64 {
        self.lock().clock.now()
    }

    /// Block sleeper `id` until the wall clock reaches `target` and, during
    /// a warm start, the barrier has passed it. A target at or before the
    /// warm barrier (always in the past) returns at once without locking.
    pub fn park(&self, id: u64, target: f64) -> Result<(), Stopped> {
        if self.is_stopped() {
            return Err(Stopped);
        }
        let barrier = f64::from_bits(self.barrier.load(Ordering::Acquire));
        if target <= barrier.min(self.origin) {
            return Ok(());
        }
        let mut st = self.lock();
        st.parked.insert(id, target);
        self.ended(&mut st);
        let result = loop {
            if st.stopped {
                break Err(Stopped);
            }
            let now = st.clock.now();
            if target <= st.barrier && target <= now {
                break Ok(());
            }
            let timed = target <= st.barrier && matches!(st.clock, Clock::System);
            st = if timed {
                let secs = (target - now).clamp(0.0, MAX_WAIT.as_secs_f64());
                let wait = Duration::from_secs_f64(secs);
                self.wake
                    .wait_timeout(st, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            } else {
                self.wake.wait(st).unwrap_or_else(|e| e.into_inner())
            };
        };
        st.parked.remove(&id);
        st.running += 1;
        result
    }

    /// Let warm sims run up to wall time `barrier`.
    pub fn set_barrier(&self, barrier: f64) {
        let mut st = self.lock();
        st.barrier = barrier;
        self.barrier.store(barrier.to_bits(), Ordering::Release);
        self.wake.notify_all();
    }

    /// Wait until nothing is running and no sleeper is due, or the engine
    /// stopped.
    pub fn settle(&self) {
        let mut st = self.lock();
        while !st.stopped && (st.running > 0 || st.any_due()) {
            st = self.wait_idle(st);
        }
    }

    fn wait_idle<'a>(&self, st: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.idle
            .wait_timeout(st, SETTLE_POLL)
            .unwrap_or_else(|e| e.into_inner())
            .0
    }

    /// Move a manual clock forward and wake sleepers. `false` on the system
    /// clock.
    pub fn advance(&self, secs: f64) -> bool {
        let mut st = self.lock();
        let Clock::Manual(t) = st.clock else {
            return false;
        };
        st.clock = Clock::Manual(t + secs);
        self.wake.notify_all();
        true
    }

    /// A sim or the driver stopped running, to park or for good. Wakes
    /// `settle` once nothing runs.
    pub fn ended(&self, st: &mut State) {
        st.running -= 1;
        if st.running == 0 {
            self.idle.notify_all();
        }
    }

    /// Raise the stop flag and wake every sleeper.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let mut st = self.lock();
        st.stopped = true;
        self.wake.notify_all();
        self.idle.notify_all();
    }

    /// Wait until every sim thread has ended.
    pub fn wait_no_threads(&self) {
        let mut st = self.lock();
        while st.threads > 0 {
            st = self.wait_idle(st);
        }
    }
}
