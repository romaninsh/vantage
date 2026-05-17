use std::time::Duration;

/// Per-Lens default policies inherited by every Dio it produces.
#[derive(Debug, Clone)]
pub struct LensDefaults {
    /// How often the refresh task fires `on_refresh`. `None` disables
    /// scheduled refresh — callers may still invoke `Dio::refresh()`
    /// manually.
    pub refresh_interval: Option<Duration>,

    /// Maximum age a cache entry may reach before counting as stale.
    /// `None` means cache entries never expire on their own.
    pub cache_ttl: Option<Duration>,

    /// Bounded mpsc capacity for the per-Dio write queue. Writes past
    /// the cap block the caller — intentional, surfaces overload.
    pub write_queue_capacity: usize,

    /// When `true`, `make_dio` awaits the `on_start` callback before
    /// returning the Dio. When `false`, `on_start` fires in the
    /// background and the Dio is returned immediately.
    pub on_start_blocking: bool,
}

impl Default for LensDefaults {
    fn default() -> Self {
        Self {
            refresh_interval: None,
            cache_ttl: None,
            write_queue_capacity: 256,
            on_start_blocking: true,
        }
    }
}
