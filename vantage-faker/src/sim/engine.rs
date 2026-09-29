//! The engine handle.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::builder::SimEngineBuilder;
use super::kind::Inner;
use super::stats::{Counters, SimStats};

/// Runs the sims of one datasource. Dropping it stops every sim and joins
/// their threads.
pub struct SimEngine {
    inner: Arc<Inner>,
    driver: Mutex<Option<JoinHandle<()>>>,
}

impl SimEngine {
    pub(super) fn new(inner: Arc<Inner>, driver: Option<JoinHandle<()>>) -> Self {
        Self {
            inner,
            driver: Mutex::new(driver),
        }
    }

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

    /// Live sims and the engine's running totals.
    pub fn stats(&self) -> SimStats {
        let c = &self.inner.counters;
        SimStats {
            live: self.live(),
            spawned: Counters::read(&c.spawned),
            ended: Counters::read(&c.ended),
            errored: Counters::read(&c.errored),
            writes: Counters::read(&c.writes),
        }
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
        self.inner.stop_sims();
    }
}

impl Drop for SimEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
