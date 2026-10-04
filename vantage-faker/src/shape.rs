//! Backend personality: a [`TableShell`] decorator that makes an in-memory
//! store behave like any real backend — paged or cursor-driven, sluggish or
//! flaky, honest or lying.
//!
//! [`BackendShape`] is the single place a scenario's personality is written
//! down; [`ShapedShell`] enforces it. The shell *advertises* exactly the
//! capabilities the shape lists and *refuses* everything else through the
//! standard [`TableShell::default_error`] discipline, so a consumer that
//! ignores capability flags fails loudly instead of silently working against
//! a backend that production will not provide.
//!
//! Everything nondeterministic (latency jitter, fault draws, boundary skew)
//! draws from one seeded rng, so a scenario with a `seed` replays
//! identically. Time-based behavior (the offline schedule, cursor expiry)
//! reads the tokio clock, so tests drive it with `tokio::time::advance`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use fake::rand::{RngExt as _, SeedableRng as _};
use tokio::time::Instant;
use vantage_core::{Result, error};
use vantage_vista::Vista;
use vantage_vista::capabilities::VistaCapabilities;
use vantage_vista::source::TableShell;

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

    fn draw(&self, rng: &mut StdRng) -> Duration {
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
    /// A `fetch_next` token older than this is dead (the 410 case).
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
    /// The server's page for `fetch_page`/`fetch_next` (and the starting
    /// size where `can_set_page_size` allows negotiation).
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
            capabilities: VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                can_order: true,
                can_search: true,
                can_subscribe: true,
                ..VistaCapabilities::default()
            },
            page_size: 25,
            latency: LatencyModel::default(),
            faults: FaultSchedule::default(),
            seed: None,
        }
    }
}

/// Which latency band a request draws from.
#[derive(Clone, Copy)]
enum OpClass {
    List,
    Get,
    Window,
    Count,
}

/// [`TableShell`] decorator enforcing a [`BackendShape`] over an inner shell
/// (in practice a store-backed shell sharing the same table).
pub struct ShapedShell {
    inner: Box<dyn TableShell>,
    shape: Arc<BackendShape>,
    /// What this shell actually advertises: the shape's list, with
    /// `can_subscribe` narrowed to what the wrapped shell truly supports — a
    /// shape may only remove capabilities, never grant one the inner shell
    /// lacks.
    capabilities: VistaCapabilities,
    /// One stream for jitter and fault draws, shared across clones so a
    /// seeded run is a single deterministic sequence.
    rng: Arc<Mutex<StdRng>>,
    /// Time zero for the offline schedule and cursor-token ages.
    epoch: Instant,
    /// Current negotiated page size — per handle, like any query state.
    page_size: usize,
    /// Whether a search filter is active on THIS handle (adds
    /// `search_extra` latency to list/window tolls). Arc'd only so `&self`
    /// async methods can observe what `&mut self` setters flipped.
    searching: Arc<AtomicBool>,
}

impl ShapedShell {
    pub fn new(inner: Box<dyn TableShell>, shape: BackendShape) -> Self {
        // Decorrelate from ValueGen's stream: same seed must not make the
        // 3rd fault draw equal the 3rd generated cell.
        let rng = match shape.seed {
            Some(seed) => StdRng::seed_from_u64(seed ^ 0x5AAD_F00D_u64),
            None => crate::value_gen::entropy_rng(),
        };
        let page_size = shape.page_size.max(1);
        let mut capabilities = shape.capabilities.clone();
        capabilities.can_subscribe &= inner.capabilities().can_subscribe;
        Self {
            inner,
            shape: Arc::new(shape),
            capabilities,
            rng: Arc::new(Mutex::new(rng)),
            epoch: Instant::now(),
            page_size,
            searching: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Pay a request's toll: scheduled outage, then latency, then the error
    /// draw — an offline backend refuses fast, a flaky one fails slowly,
    /// like their real counterparts.
    ///
    /// Every tolled request logs at debug under `vantage_faker::shape` —
    /// the tap the select tester's request accounting reads.
    async fn toll(&self, class: OpClass) -> Result<()> {
        if let Some(off) = self.shape.faults.offline {
            let elapsed = self.epoch.elapsed();
            let period = off.period.max(off.down);
            let into_period =
                Duration::from_nanos((elapsed.as_nanos() % period.as_nanos().max(1)) as u64);
            if into_period >= period.saturating_sub(off.down) {
                return Err(error!("shaped source is offline (scheduled outage)"));
            }
        }

        let (delay, failed) = {
            let rng = &mut *self.rng.lock().unwrap();
            let band = match class {
                OpClass::List => self.shape.latency.list,
                OpClass::Get => self.shape.latency.get,
                OpClass::Window => self.shape.latency.window,
                OpClass::Count => self.shape.latency.count,
            };
            let mut delay = band.map(|b| b.draw(rng)).unwrap_or_default();
            if matches!(class, OpClass::List | OpClass::Window)
                && self.searching.load(Ordering::Relaxed)
                && let Some(extra) = self.shape.latency.search_extra
            {
                delay += extra.draw(rng);
            }
            let failed = self.shape.faults.error_rate > 0.0
                && rng.random_range(0.0..1.0) < self.shape.faults.error_rate;
            (delay, failed)
        };

        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if failed {
            return Err(error!("shaped source request failed (injected fault)"));
        }
        Ok(())
    }

    /// Offset after the boundary-skew vice: occasionally ±1, producing the
    /// duplicated or missing row of offset pagination under churn.
    fn skewed_offset(&self, offset: usize) -> usize {
        if !self.shape.faults.boundary_skew || offset == 0 {
            return offset;
        }
        let draw: f64 = self.rng.lock().unwrap().random_range(0.0..1.0);
        if draw < 0.2 {
            offset - 1 // last row of the previous window repeats
        } else if draw < 0.4 {
            offset + 1 // one row is silently skipped
        } else {
            offset
        }
    }

    /// Reported total = truth + the configured lie, clamped to zero.
    async fn lied_total(&self, vista: &Vista) -> Result<i64> {
        let truth = self.inner.get_vista_count(vista).await?;
        Ok((truth + self.shape.faults.total_lie).max(0))
    }

    fn cursor_token(&self, offset: usize) -> CborValue {
        CborValue::Array(vec![
            CborValue::Integer((offset as i64).into()),
            CborValue::Integer((self.epoch.elapsed().as_millis() as i64).into()),
        ])
    }

    fn decode_cursor(&self, token: &CborValue) -> Result<usize> {
        let CborValue::Array(parts) = token else {
            return Err(error!("shaped source: malformed cursor token"));
        };
        let (Some(CborValue::Integer(offset)), Some(CborValue::Integer(issued_ms))) =
            (parts.first(), parts.get(1))
        else {
            return Err(error!("shaped source: malformed cursor token"));
        };
        if let Some(expiry) = self.shape.faults.cursor_expiry {
            let issued = Duration::from_millis(i128::from(*issued_ms).max(0) as u64);
            if self.epoch.elapsed().saturating_sub(issued) > expiry {
                return Err(error!("shaped source: cursor token expired"));
            }
        }
        Ok(i128::from(*offset).max(0) as usize)
    }

    fn gate(&self, allowed: bool, method: &str, capability: &str) -> Result<()> {
        if allowed {
            Ok(())
        } else {
            Err(self.default_error(method, capability))
        }
    }
}

mod shell;
