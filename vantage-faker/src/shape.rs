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

use async_trait::async_trait;
use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use fake::rand::{RngExt as _, SeedableRng as _};
use indexmap::IndexMap;
use tokio::time::Instant;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::capabilities::VistaCapabilities;
use vantage_vista::column::Column;
use vantage_vista::reference::Reference;
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

/// Undeclared payload riding along on every record: `count` extra fields of
/// `size`-char strings — the fat API response the query didn't ask for.
/// Applied at generation time (see `FakerCtx`), recorded here so a shape is
/// the complete personality description.
#[derive(Clone, Copy, Debug)]
pub struct ExtraFields {
    pub count: usize,
    pub size: usize,
}

/// The complete personality of one shaped backend.
#[derive(Clone, Debug)]
pub struct BackendShape {
    /// Exactly what the backend advertises; everything else is refused.
    pub capabilities: VistaCapabilities,
    /// The server's page for `fetch_page`/`fetch_next` (and the starting
    /// size where `can_set_page_size` allows negotiation).
    pub page_size: usize,
    pub latency: LatencyModel,
    pub faults: FaultSchedule,
    pub extra_fields: Option<ExtraFields>,
    /// Fraction of generated string cells drawn from the anomaly pool.
    pub weirdness: f64,
    /// Deterministic replay when set — drives value generation, fault draws
    /// and latency jitter alike.
    pub seed: Option<u64>,
}

impl Default for BackendShape {
    /// The classic `MockShell` profile: full CRUD, order, search, no paging,
    /// no faults, instant.
    fn default() -> Self {
        Self {
            capabilities: VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                can_order: true,
                can_search: true,
                ..VistaCapabilities::default()
            },
            page_size: 25,
            latency: LatencyModel::default(),
            faults: FaultSchedule::default(),
            extra_fields: None,
            weirdness: 0.0,
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
/// (in practice a `MockShell` clone sharing the effect's store).
pub struct ShapedShell {
    inner: Box<dyn TableShell>,
    shape: Arc<BackendShape>,
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
        Self {
            inner,
            shape: Arc::new(shape),
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

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for ShapedShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }

    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.shape.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "faker-shaped"
    }

    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        tracing::debug!(target: "vantage_faker::shape", op = "list", "request");
        self.toll(OpClass::List).await?;
        self.inner.list_vista_values(vista).await
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        tracing::debug!(target: "vantage_faker::shape", op = "get", id = %id, "request");
        self.toll(OpClass::Get).await?;
        self.inner.get_vista_value(vista, id).await
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.toll(OpClass::Get).await?;
        self.inner.get_vista_some_value(vista).await
    }

    async fn fetch_window(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.gate(
            self.shape.capabilities.can_fetch_window,
            "fetch_window",
            "can_fetch_window",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "window", offset, limit, "request");
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(offset);
        self.inner.fetch_window(vista, offset, limit).await
    }

    /// The total rides the same response envelope as the rows (no second
    /// toll) — and only exists where the shape can count.
    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<i64>)> {
        let rows = self.fetch_window(vista, offset, limit).await?;
        let total = if self.shape.capabilities.can_count {
            Some(self.lied_total(vista).await?)
        } else {
            None
        };
        Ok((rows, total))
    }

    async fn fetch_page(
        &self,
        vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.gate(
            self.shape.capabilities.can_fetch_page,
            "fetch_page",
            "can_fetch_page",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(page.saturating_sub(1) * self.page_size);
        self.inner.fetch_window(vista, offset, self.page_size).await
    }

    async fn fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        self.gate(
            self.shape.capabilities.can_fetch_next,
            "fetch_next",
            "can_fetch_next",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = match &token {
            None => 0,
            Some(t) => self.decode_cursor(t)?,
        };
        let offset = self.skewed_offset(offset);
        let rows = self
            .inner
            .fetch_window(vista, offset, self.page_size)
            .await?;
        let next = (rows.len() == self.page_size).then(|| self.cursor_token(offset + rows.len()));
        Ok((rows, next))
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        self.gate(
            self.shape.capabilities.can_count,
            "get_vista_count",
            "can_count",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "count", "request");
        self.toll(OpClass::Count).await?;
        self.lied_total(vista).await
    }

    // ---- Writes: forwarded when advertised, no toll — the scenarios stress
    // the read path; write latency is a personality nobody asked for yet.

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_value",
            "can_insert",
        )?;
        self.inner.insert_vista_value(vista, id, record).await
    }

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_update,
            "replace_vista_value",
            "can_update",
        )?;
        self.inner.replace_vista_value(vista, id, record).await
    }

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_update,
            "patch_vista_value",
            "can_update",
        )?;
        self.inner.patch_vista_value(vista, id, partial).await
    }

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_value",
            "can_delete",
        )?;
        self.inner.delete_vista_value(vista, id).await
    }

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_all_values",
            "can_delete",
        )?;
        self.inner.delete_vista_all_values(vista).await
    }

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_return_id_value",
            "can_insert",
        )?;
        self.inner.insert_vista_return_id_value(vista, record).await
    }

    // ---- Query state ------------------------------------------------------

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Equality push-down is universal — not gated, per the capability
        // contract.
        self.inner.add_eq_condition(field, value)
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "add_search",
            "can_search",
        )?;
        self.inner.add_search(text)?;
        self.searching.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "clear_search",
            "can_search",
        )?;
        self.inner.clear_search()?;
        self.searching.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: vantage_vista::sort::SortDirection) -> Result<()> {
        self.gate(self.shape.capabilities.can_order, "add_order", "can_order")?;
        self.inner.add_order(field, dir)
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_order,
            "clear_orders",
            "can_order",
        )?;
        self.inner.clear_orders()
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_set_page_size,
            "set_page_size",
            "can_set_page_size",
        )?;
        self.page_size = size.max(1);
        Ok(())
    }

    /// Same store and fault/jitter stream, fresh query state — the clone a
    /// consumer narrows (`add_order` + `fetch_window`) without disturbing
    /// this handle. Each clone tracks its own `searching`.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(Self {
            inner,
            shape: self.shape.clone(),
            rng: self.rng.clone(),
            epoch: self.epoch,
            page_size: self.page_size,
            searching: Arc::new(AtomicBool::new(false)),
        }))
    }
}
