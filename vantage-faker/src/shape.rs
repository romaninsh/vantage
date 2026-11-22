//! Backend personality: a [`TableShell`] decorator that makes an in-memory
//! store behave like any real backend — paged or cursor-driven, sluggish or
//! flaky, honest or lying.
//!
//! [`BackendShape`] is the single place a scenario's personality is written
//! down; [`ShapedShell`] enforces it. The shell *advertises* exactly the
//! capabilities the shape lists and *refuses* everything else with
//! [`unsupported`], so a consumer that ignores capability flags fails loudly
//! instead of silently working against a backend that production will not
//! provide.
//!
//! Wrap a Vista's shell with [`Vista::map_shell`](vantage_vista::Vista::map_shell)
//! so the Vista's name and applied query survive:
//! `vista.map_shell(|inner| ShapedShell::new(inner, shape))`.
//!
//! Everything nondeterministic (latency jitter, fault draws, boundary skew)
//! draws from one seeded rng, so a scenario with a `seed` replays
//! identically. Time-based behavior (the offline schedule, cursor expiry)
//! reads the tokio clock, so tests drive it with `tokio::time::advance`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use fake::rand::{RngExt as _, SeedableRng as _};
use tokio::time::Instant;
use vantage_core::{Result, error};
use vantage_dataset::Inserted;
use vantage_types::Record;
use vantage_vista::{Capability, TableShell, VistaCapabilities, unsupported};

mod profile;
mod shell;

pub use profile::{BackendShape, FaultSchedule, Latency, LatencyModel, Offline, WriteRefusal};

/// One fault draw at `rate`; a zero rate leaves the stream untouched.
fn hit(rng: &mut StdRng, rate: f64) -> bool {
    rate > 0.0 && rng.random_range(0.0..1.0) < rate
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
///
/// A clone shares the shape, the fault/jitter stream and the epoch, and
/// carries its own copy of the query state: the inner shell's narrowing and
/// whether a search is active.
#[derive(Clone)]
pub struct ShapedShell {
    inner: Box<dyn TableShell>,
    shape: Arc<BackendShape>,
    /// What this shell actually advertises; see [`Self::new`].
    capabilities: VistaCapabilities,
    /// One stream for jitter and fault draws, shared across clones so a
    /// seeded run is a single deterministic sequence.
    rng: Arc<Mutex<StdRng>>,
    /// Time zero for the offline schedule and cursor-token ages.
    epoch: Instant,
    /// Whether a search filter is active on this handle (adds
    /// `search_extra` latency to list/window tolls).
    searching: bool,
}

impl ShapedShell {
    /// Shape `inner`. The shell advertises the shape's capabilities that the
    /// inner shell also has: a shape may only remove capabilities, never
    /// grant one the inner shell lacks. Two exceptions: `Cursor` is served
    /// by the shape itself over the inner shell's windows, so it needs the
    /// inner `Window`; and `ConfineWrites` is the inner shell's, since every
    /// write is forwarded to it. `Import` and `Traverse` are never
    /// advertised: bulk import is not part of any shape, and a reference's
    /// target would not be shaped.
    pub fn new(inner: Box<dyn TableShell>, shape: BackendShape) -> Self {
        // Decorrelate from ValueGen's stream: same seed must not make the
        // 3rd fault draw equal the 3rd generated cell.
        let rng = match shape.seed {
            Some(seed) => StdRng::seed_from_u64(seed ^ 0x5AAD_F00D_u64),
            None => crate::value_gen::entropy_rng(),
        };
        let theirs = *inner.capabilities();
        let capabilities = Capability::iter()
            .filter(|&c| match c {
                Capability::Cursor => shape.capabilities.has(c) && theirs.has(Capability::Window),
                Capability::ConfineWrites => theirs.has(c),
                Capability::Import | Capability::Traverse => false,
                _ => shape.capabilities.has(c) && theirs.has(c),
            })
            .collect();
        Self {
            inner,
            shape: Arc::new(shape),
            capabilities,
            rng: Arc::new(Mutex::new(rng)),
            epoch: Instant::now(),
            searching: false,
        }
    }

    /// The server's cursor page.
    fn page_size(&self) -> usize {
        self.shape.page_size.max(1)
    }

    /// Pay a request's toll: scheduled outage, then latency, then the error
    /// draw — an offline backend refuses fast, a flaky one fails slowly,
    /// like their real counterparts.
    ///
    /// Every tolled request logs at debug under `vantage_faker::shape` —
    /// the tap the select tester's request accounting reads.
    async fn toll(&self, class: OpClass) -> Result<()> {
        self.check_online()?;

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
                && self.searching
                && let Some(extra) = self.shape.latency.search_extra
            {
                delay += extra.draw(rng);
            }
            (delay, hit(rng, self.shape.faults.error_rate))
        };

        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if failed {
            return Err(error!("shaped source request failed (injected fault)").mark_unavailable());
        }
        Ok(())
    }

    fn check_online(&self) -> Result<()> {
        if let Some(off) = self.shape.faults.offline {
            let elapsed = self.epoch.elapsed();
            let period = off.period.max(off.down);
            let into_period =
                Duration::from_nanos((elapsed.as_nanos() % period.as_nanos().max(1)) as u64);
            if into_period >= period.saturating_sub(off.down) {
                return Err(
                    error!("shaped source is offline (scheduled outage)").mark_unavailable()
                );
            }
        }
        Ok(())
    }

    /// A write's toll, before it reaches the store: outage, refusal,
    /// latency, then the write error draw.
    async fn write_toll(&self) -> Result<()> {
        self.check_online()?;
        if let Some(refusal) = self.shape.faults.refuse {
            return Err(match refusal {
                WriteRefusal::Conflict => {
                    error!("shaped source refuses writes (injected conflict)").mark_conflict()
                }
                WriteRefusal::InvalidValue => {
                    error!("shaped source refuses writes (injected invalid value)")
                        .mark_invalid_value()
                }
                WriteRefusal::Forbidden => {
                    error!("shaped source refuses writes (injected permission denial)")
                        .mark_forbidden()
                }
            });
        }

        let (delay, failed) = {
            let rng = &mut *self.rng.lock().unwrap();
            let delay = self
                .shape
                .latency
                .write
                .map(|b| b.draw(rng))
                .unwrap_or_default();
            (delay, hit(rng, self.shape.faults.write_error_rate))
        };

        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if failed {
            return Err(error!("shaped source write failed (injected fault)").mark_unavailable());
        }
        Ok(())
    }

    /// The write has landed; its answer, unless the lost-reply draw drops it.
    fn reply<T>(&self, answer: T) -> Result<T> {
        if hit(
            &mut self.rng.lock().unwrap(),
            self.shape.faults.lost_reply_rate,
        ) {
            return Err(
                error!("shaped source lost the write's reply (injected fault)").mark_unavailable(),
            );
        }
        Ok(answer)
    }

    /// A queued insert: answered `Accepted` now, stored in the inner shell
    /// after `delay`. A caller without an id gets one drawn from the shared
    /// rng, since the queue must name the row before it exists.
    fn enqueue_insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
        delay: Duration,
    ) -> (String, Inserted<CborValue>) {
        let id = match id {
            Some(id) => id.to_string(),
            None => format!(
                "{:016x}",
                self.rng.lock().unwrap().random_range(0..=u64::MAX)
            ),
        };
        let inner = self.inner.clone();
        let record = record.clone();
        let queued = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Err(e) = inner.insert(Some(queued.as_str()), &record).await {
                tracing::warn!(
                    target: "vantage_faker::shape",
                    id = %queued,
                    "queued insert failed: {e}"
                );
            }
        });
        (id, Inserted::Accepted)
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
    fn lied(&self, truth: i64) -> i64 {
        (truth + self.shape.faults.total_lie).max(0)
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

    /// `Ok` when this shell advertises `capability`, its refusal otherwise.
    fn gate(&self, capability: Capability) -> Result<()> {
        if self.capabilities.has(capability) {
            Ok(())
        } else {
            unsupported(capability)
        }
    }
}
