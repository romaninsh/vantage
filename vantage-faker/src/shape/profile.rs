//! [`BackendShape`] and its parts: the personality a [`ShapedShell`]
//! enforces.
//!
//! [`ShapedShell`]: super::ShapedShell

use std::time::Duration;

use fake::rand::RngExt as _;
use fake::rand::rngs::StdRng;
use vantage_vista::{Capability, VistaCapabilities};

/// A latency band: each request draws uniformly from `[min, max]`.
#[derive(Clone, Copy, Debug)]
pub struct Latency {
    pub min: Duration,
    pub max: Duration,
}

impl Latency {
    pub fn fixed(d: Duration) -> Self {
        Self { min: d, max: d }
    }

    pub fn between(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max: max.max(min),
        }
    }

    pub(super) fn draw(&self, rng: &mut StdRng) -> Duration {
        if self.max <= self.min {
            return self.min;
        }
        let spread = (self.max - self.min).as_millis() as u64;
        self.min + Duration::from_millis(rng.random_range(0..=spread))
    }
}

/// Per-operation-class latency. Absent class = instant. Real backends are
/// asymmetric — a fast get-by-id next to a slow list is a personality, not
/// an accident — so each class is tuned independently.
#[derive(Clone, Debug, Default)]
pub struct LatencyModel {
    pub list: Option<Latency>,
    pub get: Option<Latency>,
    pub window: Option<Latency>,
    pub count: Option<Latency>,
    /// Added on top of `list`/`window` while a search filter is active —
    /// the "search is the expensive endpoint" personality.
    pub search_extra: Option<Latency>,
}

/// Scheduled unavailability: within every `period`, the backend is down for
/// the final `down`. The window starts *online* so a scenario's first paint
/// works, then the outage arrives on schedule.
#[derive(Clone, Copy, Debug)]
pub struct Offline {
    pub down: Duration,
    pub period: Duration,
}

/// The vices. All default off.
#[derive(Clone, Debug, Default)]
pub struct FaultSchedule {
    /// Fraction of tolled requests failing with an injected error.
    pub error_rate: f64,
    pub offline: Option<Offline>,
    /// A `fetch_cursor` token older than this is dead (the 410 case).
    pub cursor_expiry: Option<Duration>,
    /// Reported totals are off by this much from the truth (clamped ≥ 0).
    pub total_lie: i64,
    /// Windowed fetches occasionally shift their offset by ±1 — the
    /// duplicate/missing row an offset-paginated API produces under churn.
    pub boundary_skew: bool,
}

/// The transport personality of one shaped backend: what it advertises, how
/// it pages, how slow and how faulty it is. Row content — weirdness, extra
/// fields — is a generation setting on
/// [`TableGen`](crate::TableGen), not part of the shape.
#[derive(Clone, Debug)]
pub struct BackendShape {
    /// Exactly what the backend advertises; everything else is refused.
    pub capabilities: VistaCapabilities,
    /// The server's largest cursor page: `fetch_cursor` serves at most this
    /// many rows, however many the caller asks for.
    pub page_size: usize,
    pub latency: LatencyModel,
    pub faults: FaultSchedule,
    /// Deterministic replay of latency jitter, fault draws and boundary
    /// skew when set; fresh entropy when `None`. Does not affect row values.
    pub seed: Option<u64>,
}

impl Default for BackendShape {
    /// The classic in-memory profile: full CRUD, order, search, watch, no
    /// paging, no faults, instant.
    fn default() -> Self {
        Self {
            capabilities: [
                Capability::Insert,
                Capability::Update,
                Capability::Delete,
                Capability::Order,
                Capability::Search,
                Capability::Subscribe,
            ]
            .into_iter()
            .collect(),
            page_size: 25,
            latency: LatencyModel::default(),
            faults: FaultSchedule::default(),
            seed: None,
        }
    }
}
