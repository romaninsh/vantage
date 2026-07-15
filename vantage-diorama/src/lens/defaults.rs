use std::time::Duration;

/// Per-Lens default policies inherited by every Dio it produces.
#[derive(Debug, Clone)]
pub struct LensDefaults {
    /// How often the refresh task fires `on_refresh`. `None` disables
    /// scheduled refresh — callers may still invoke `Dio::refresh()`
    /// manually.
    pub refresh_interval: Option<Duration>,

    /// Slower refresh interval used while the app is on
    /// [`Standby`](crate::Activity::Standby). `None` falls back to
    /// `refresh_interval`. While [`Offline`](crate::Activity::Offline) the
    /// scheduler skips the refresh body entirely until the app is back.
    pub standby_refresh_interval: Option<Duration>,

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

    /// When `true`, opening a `TableScenery` automatically schedules a
    /// `set_viewport(0..page_size)` so the configured `on_load_chunk`
    /// re-fetches the first page in the background. Cached rows (if
    /// any) are painted immediately, then repainted as the fresh
    /// chunk lands. Turn off for read-only / offline modes.
    pub refresh_on_open: bool,

    /// Window over which rapid `set_viewport` calls are coalesced
    /// before a chunk fetch is fired.
    pub viewport_debounce: Duration,

    /// Size of the per-Dio augment-scheduler worker pool — how many
    /// per-row detail fetches may run concurrently. The default of 1
    /// keeps fetch order deterministic (round-robin across the views
    /// demanding rows); raise it when the detail source tolerates
    /// parallel requests and hydration latency matters more than order.
    pub augment_workers: usize,
}

impl Default for LensDefaults {
    fn default() -> Self {
        Self {
            refresh_interval: None,
            standby_refresh_interval: None,
            cache_ttl: None,
            write_queue_capacity: 256,
            on_start_blocking: true,
            refresh_on_open: true,
            viewport_debounce: Duration::from_millis(50),
            augment_workers: 1,
        }
    }
}
