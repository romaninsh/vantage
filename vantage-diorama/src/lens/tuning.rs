//! [`Tuning`]: the knobs every Dio of a Lens shares.

use std::time::Duration;

use vantage_core::{Result, error};

/// Per-Lens settings inherited by every Dio it builds.
///
/// Start from [`Tuning::default`] and change fields with the `with_*`
/// setters.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Tuning {
    /// Capacity of each Dio's write queue, which runs the Dio's writes one at
    /// a time. A write past it waits for room before it is staged, which
    /// surfaces overload instead of hiding it. Must be non-zero.
    pub write_queue: usize,
    /// Window over which rapid `set_viewport` calls coalesce into one fetch.
    pub viewport_debounce: Duration,
    /// Concurrent per-row augmentation fetches per Dio. One keeps the fetch
    /// order deterministic (round-robin across the views asking); raise it
    /// when the detail source tolerates parallel requests. Must be non-zero.
    pub augment_workers: usize,
    /// Whether a paged table scenery re-fetches its first window from the
    /// master when it opens, replacing whatever the cache showed. Turn off
    /// for read-only or offline use.
    pub refresh_on_open: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            write_queue: 256,
            viewport_debounce: Duration::from_millis(50),
            augment_workers: 1,
            refresh_on_open: true,
        }
    }
}

impl Tuning {
    pub fn with_write_queue(mut self, n: usize) -> Self {
        self.write_queue = n;
        self
    }

    pub fn with_viewport_debounce(mut self, window: Duration) -> Self {
        self.viewport_debounce = window;
        self
    }

    pub fn with_augment_workers(mut self, n: usize) -> Self {
        self.augment_workers = n;
        self
    }

    pub fn with_refresh_on_open(mut self, enabled: bool) -> Self {
        self.refresh_on_open = enabled;
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.write_queue == 0 {
            return Err(error!(
                "Tuning::write_queue must be non-zero",
                setting = "tuning.write_queue"
            ));
        }
        if self.augment_workers == 0 {
            return Err(error!(
                "Tuning::augment_workers must be non-zero",
                setting = "tuning.augment_workers"
            ));
        }
        Ok(())
    }
}
