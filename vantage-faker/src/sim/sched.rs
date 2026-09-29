//! Shared scheduling state: who is running, who sleeps until when, the warm
//! barrier and the stop flag.
//!
//! Each sleeper parks its own thread. Whoever makes it due (a raised
//! barrier, a manual clock advance, a stop) takes it off the parked list,
//! counts it as running again and unparks exactly that thread; on the
//! system clock a sleeper also wakes itself when its deadline passes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::Thread;
use std::time::Duration;

use super::clock::Clock;

/// Longest single timed park; a sleeper re-checks the clock at least this
/// often.
const MAX_WAIT: Duration = Duration::from_secs(3600);
/// How often `settle` re-checks for sleepers that became due on the system
/// clock, which wakes nobody.
const SETTLE_POLL: Duration = Duration::from_millis(20);

const WAITING: u8 = 0;
const WOKEN: u8 = 1;
const STOPPED: u8 = 2;

/// The engine is stopping; the sleeper must end its script.
#[derive(Debug)]
pub(super) struct Stopped;

/// A parked sleeper: its deadline, its thread and how it was woken.
struct Waiter {
    target: f64,
    thread: Thread,
    state: Arc<AtomicU8>,
}

impl Waiter {
    fn wake(self, how: u8) {
        self.state.store(how, Ordering::Release);
        self.thread.unpark();
    }
}

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
    /// Parked sims (and the driver) by id.
    parked: HashMap<u64, Waiter>,
}

impl State {
    /// Sleepers with a target at or before this may run.
    fn limit(&self) -> f64 {
        self.barrier.min(self.clock.now())
    }

    fn any_due(&self) -> bool {
        let limit = self.limit();
        self.parked.values().any(|w| w.target <= limit)
    }

    /// Wake every due sleeper; each counts as running again.
    fn wake_due(&mut self) {
        let limit = self.limit();
        let due: Vec<u64> = self
            .parked
            .iter()
            .filter(|(_, w)| w.target <= limit)
            .map(|(id, _)| *id)
            .collect();
        self.running += due.len();
        for id in due {
            if let Some(w) = self.parked.remove(&id) {
                w.wake(WOKEN);
            }
        }
    }
}

pub(super) struct Sched {
    state: Mutex<State>,
    /// Wakes `settle` and `stop` when nothing runs or a thread ends.
    idle: Condvar,
    stop: Arc<AtomicBool>,
    /// `State::barrier` as f64 bits, for the lock-free fast path of `park`.
    barrier: AtomicU64,
    /// Wall time the engine started at; the warm barrier never passes it.
    origin: f64,
    /// Whether the clock is the system clock, which sleepers read unlocked.
    system: Option<Clock>,
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
            idle: Condvar::new(),
            stop,
            barrier: AtomicU64::new(f64::INFINITY.to_bits()),
            origin,
            system: matches!(clock, Clock::System { .. }).then_some(clock),
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
        let state = Arc::new(AtomicU8::new(WAITING));
        {
            let mut st = self.lock();
            if st.stopped {
                return Err(Stopped);
            }
            if target <= st.limit() {
                return Ok(());
            }
            let waiter = Waiter {
                target,
                thread: std::thread::current(),
                state: state.clone(),
            };
            st.parked.insert(id, waiter);
            self.ended(&mut st);
        }
        loop {
            match state.load(Ordering::Acquire) {
                WOKEN => return Ok(()),
                STOPPED => return Err(Stopped),
                _ => {}
            }
            // On the system clock a sleeper wakes itself at its deadline; a
            // deadline that passed since it registered (the clock moves
            // between the check under the lock and here) is due now, and
            // must not park untimed — nothing else wakes a live sleeper.
            // A manual clock's sleepers are woken by whoever makes them due.
            let left = self.system.map(|c| target - c.now());
            match left {
                Some(secs) if secs > 0.0 => std::thread::park_timeout(Duration::from_secs_f64(
                    secs.min(MAX_WAIT.as_secs_f64()),
                )),
                Some(_) => {}
                None => {
                    std::thread::park();
                    continue;
                }
            }
            let mut st = self.lock();
            if state.load(Ordering::Acquire) == WAITING && target <= st.limit() {
                st.parked.remove(&id);
                st.running += 1;
                return Ok(());
            }
            // Past its deadline but held back by the warm barrier: wait for
            // the barrier to move (raising it wakes due sleepers) instead of
            // spinning. An unpark that lands first is kept for this park.
            if left.is_some_and(|secs| secs <= 0.0) {
                drop(st);
                std::thread::park();
            }
        }
    }

    /// Let warm sims run up to wall time `barrier`.
    pub fn set_barrier(&self, barrier: f64) {
        let mut st = self.lock();
        st.barrier = barrier;
        self.barrier.store(barrier.to_bits(), Ordering::Release);
        st.wake_due();
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

    /// Move a manual clock forward and wake the sleepers now due. `false`
    /// on the system clock.
    pub fn advance(&self, secs: f64) -> bool {
        let mut st = self.lock();
        let Clock::Manual(t) = st.clock else {
            return false;
        };
        st.clock = Clock::Manual(t + secs);
        st.wake_due();
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

    /// A sim thread is gone.
    pub fn thread_ended(&self, st: &mut State) {
        st.threads -= 1;
        if st.threads == 0 {
            self.idle.notify_all();
        }
    }

    /// Raise the stop flag and wake every sleeper.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let mut st = self.lock();
        st.stopped = true;
        let parked: Vec<Waiter> = st.parked.drain().map(|(_, w)| w).collect();
        st.running += parked.len();
        for w in parked {
            w.wake(STOPPED);
        }
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
