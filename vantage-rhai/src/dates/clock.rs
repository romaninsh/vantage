//! The time `now()` and `today()` read: the system clock, or a manual one a
//! test sets and moves. A clock travels with the host that uses it, never
//! through a global, so tests can run beside a live app in one process.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use chrono::{DateTime, Months, TimeDelta, Utc};

/// Clones share one time; [`fork`](Self::fork) makes an independent copy.
///
/// ```
/// use chrono::{DateTime, TimeDelta, TimeZone, Utc};
/// use vantage_rhai::{Clock, Dates};
///
/// let at = |m, d, h| Utc.with_ymd_and_hms(2026, m, d, h, 0, 0).unwrap();
/// let clock = Clock::manual(at(1, 31, 9));
/// let mut engine = vantage_rhai::rhai::Engine::new();
/// Dates::register_with(&mut engine, clock.clone());
///
/// let what_if = clock.fork();
/// clock.advance_months(1); // clamps to 28 February
/// what_if.advance(TimeDelta::days(1));
/// let today: DateTime<Utc> = engine.eval("today()").unwrap();
/// assert_eq!(today, at(2, 28, 0));
/// assert_eq!(what_if.now(), at(2, 1, 9));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Clock(Option<Arc<Mutex<DateTime<Utc>>>>);

impl Clock {
    pub fn system() -> Self {
        Self(None)
    }

    /// A clock that stays at `start` until moved.
    pub fn manual(start: DateTime<Utc>) -> Self {
        Self(Some(Arc::new(Mutex::new(start))))
    }

    pub fn is_system(&self) -> bool {
        self.0.is_none()
    }

    pub fn now(&self) -> DateTime<Utc> {
        match &self.0 {
            Some(time) => *time.lock().unwrap_or_else(PoisonError::into_inner),
            None => DateTime::from(SystemTime::now()),
        }
    }

    /// Move a manual clock forward (or back, with a negative `by`). The
    /// system clock can't be moved: this does nothing to it, as does a move
    /// past chrono's range.
    pub fn advance(&self, by: TimeDelta) {
        self.update(|t| t.checked_add_signed(by));
    }

    /// Calendar months; a day past the target month's end clamps to its
    /// last day, as `add_months` does in scripts.
    pub fn advance_months(&self, n: u32) {
        self.update(|t| t.checked_add_months(Months::new(n)));
    }

    /// A manual clock's fork starts at its time and moves alone; the system
    /// clock's fork is the system clock.
    pub fn fork(&self) -> Clock {
        match &self.0 {
            Some(_) => Clock::manual(self.now()),
            None => Clock::system(),
        }
    }

    fn update(&self, step: impl FnOnce(DateTime<Utc>) -> Option<DateTime<Utc>>) {
        if let Some(time) = &self.0 {
            let mut time = time.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(next) = step(*time) {
                *time = next;
            }
        }
    }
}
