//! App-activity signal that drives the Lens's **adaptive** refresh cadence.
//!
//! The desktop app knows things the data layer doesn't: whether its window is
//! focused, whether the user has touched anything recently, whether the network
//! is up. It funnels that into one cheap shared handle; every Lens given the
//! handle polls fast while the app is active, slows right down on standby, and
//! stops entirely while offline (resuming on reconnect). One signal, set by the
//! UI, read by every Dio's refresh loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// What the app is currently doing, from the refresh scheduler's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Activity {
    /// Foreground + recently interacted: poll at the active interval.
    Active = 0,
    /// Backgrounded or idle: poll at the (slower) standby interval.
    Standby = 1,
    /// No network: skip polling until back online.
    Offline = 2,
}

/// A cheap, cloneable, shared handle the app updates as window focus / idle /
/// network state change. Pass one (cloned) into every Lens via
/// [`activity_signal`](crate::lens::LensBuilder::activity_signal); flipping it
/// re-paces all their refresh loops at once. Defaults to
/// [`Active`](Activity::Active).
#[derive(Debug, Clone)]
pub struct ActivitySignal(Arc<AtomicU8>);

impl ActivitySignal {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU8::new(Activity::Active as u8)))
    }

    pub fn set(&self, activity: Activity) {
        self.0.store(activity as u8, Ordering::Relaxed);
    }

    pub fn get(&self) -> Activity {
        match self.0.load(Ordering::Relaxed) {
            1 => Activity::Standby,
            2 => Activity::Offline,
            _ => Activity::Active,
        }
    }
}

impl Default for ActivitySignal {
    fn default() -> Self {
        Self::new()
    }
}
