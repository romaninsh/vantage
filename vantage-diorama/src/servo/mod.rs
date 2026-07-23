//! `Servo` — the editing companion: a servo loop over a record.
//!
//! A form is a servomechanism. `data` holds the **commanded setpoints**
//! (what the user typed), the `baseline` holds the **measured upstream
//! state** (what the vista last reported, arriving through the Dio's
//! event bus), and the **error signal** is their per-field difference —
//! computed by diff, never by interception. Untouched fields run in
//! continuous tracking: upstream changes update them live and they stay
//! clean. Touched fields lock and hold; upstream converging to the
//! setpoint zeroes the error and releases the lock on its own.
//!
//! [`flash`](Servo::flash) is the actuation: it freezes the error signal
//! at fire time into an immutable [`ChangeFlash`] carrying only the
//! changed fields, emits it through the Dio's optimistic write path, and
//! settles the servo clean. Later upstream changes or slow persistence
//! never mutate an emitted flash.
//!
//! A servo holds a **strong** Dio handle — deliberately, unlike
//! sceneries: while a form is open, the write pipeline it will flash
//! through must stay alive.

mod tracking;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use ciborium::Value as CborValue;
use tokio::sync::{broadcast, watch};
use vantage_core::Result;
use vantage_types::Record;

use crate::dio::{Dio, DioEvent, DioInner, Generation, cbor_scalar_string};
use crate::ops::{ChangeFlash, FlashKind};

/// Where the servo loop currently stands.
#[derive(Debug, Clone)]
pub enum ServoStatus {
    /// Following the measurement; no write in flight.
    Tracking,
    /// A flash was emitted and its write-through hasn't confirmed yet.
    Pending,
    /// The last flash was rejected and rolled back.
    Failed(String),
}

pub(crate) struct ServoState {
    /// Live-instance census (see [`crate::stats`]).
    _tally: crate::stats::Tally,
    id: RwLock<Option<String>>,
    baseline: RwLock<Option<Record<CborValue>>>,
    data: RwLock<Record<CborValue>>,
    status: RwLock<ServoStatus>,
    generation: AtomicU64,
    generation_tx: watch::Sender<Generation>,
}

impl ServoState {
    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send_replace(Generation(next));
    }

    fn absorb(&self, incoming: Option<Record<CborValue>>) {
        {
            let mut baseline = self.baseline.write().unwrap();
            let mut data = self.data.write().unwrap();
            tracking::absorb(&mut baseline, &mut data, incoming);
        }
        self.bump_generation();
    }

    fn set_status(&self, status: ServoStatus) {
        *self.status.write().unwrap() = status;
        self.bump_generation();
    }
}

/// The editing companion. Open one with [`Dio::servo`](crate::Dio::servo)
/// (existing record) or [`Dio::servo_new`](crate::Dio::servo_new) (insert).
pub struct Servo {
    dio: Dio,
    state: Arc<ServoState>,
    _guard: ServoGuard,
}

/// Aborts the bus-tracking task when the servo drops — a closed form
/// stops reacting instead of living for the Dio's whole lifetime.
struct ServoGuard {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ServoGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Servo {
    /// The record id this servo is bound to. `None` until an unsaved
    /// [`Dio::servo_new`](crate::Dio::servo_new) fires its first flash.
    pub fn id(&self) -> Option<String> {
        self.state.id.read().unwrap().clone()
    }

    /// Command a setpoint — the field locks and holds until the servo
    /// is actuated ([`flash`](Self::flash)), released
    /// ([`revert`](Self::revert)), or the measurement converges on it.
    pub fn set(&self, field: impl Into<String>, value: impl Into<CborValue>) {
        self.state
            .data
            .write()
            .unwrap()
            .insert(field.into(), value.into());
        self.state.bump_generation();
    }

    /// The current value of `field` — the setpoint when locked, the
    /// measurement when tracking. This is what a form renders.
    pub fn get(&self, field: &str) -> Option<CborValue> {
        self.state.data.read().unwrap().get(field).cloned()
    }

    /// The full record as currently displayed (setpoints over measurement).
    pub fn record(&self) -> Record<CborValue> {
        self.state.data.read().unwrap().clone()
    }

    /// The measured upstream state, `None` for a record that doesn't
    /// exist (yet).
    pub fn baseline(&self) -> Option<Record<CborValue>> {
        self.state.baseline.read().unwrap().clone()
    }

    /// The error signal: every field whose displayed value differs from
    /// the baseline. This — and only this — is what a flash will carry.
    pub fn error(&self) -> Record<CborValue> {
        let baseline = self.state.baseline.read().unwrap();
        let data = self.state.data.read().unwrap();
        tracking::error_of(baseline.as_ref(), &data)
    }

    /// Whether `field` currently carries error (is locked on a setpoint).
    pub fn dirty(&self, field: &str) -> bool {
        self.error().get(field).is_some()
    }

    /// Whether any field carries error.
    pub fn is_dirty(&self) -> bool {
        !self.error().is_empty()
    }

    /// Release one field back to tracking: its value returns to the
    /// baseline measurement.
    pub fn revert(&self, field: &str) {
        {
            let baseline = self.state.baseline.read().unwrap();
            let mut data = self.state.data.write().unwrap();
            match baseline.as_ref().and_then(|b| b.get(field)).cloned() {
                Some(measured) => {
                    data.insert(field.to_string(), measured);
                }
                None => {
                    data.shift_remove(field);
                }
            }
        }
        self.state.bump_generation();
    }

    /// Release every field back to tracking.
    pub fn revert_all(&self) {
        {
            let baseline = self.state.baseline.read().unwrap();
            let mut data = self.state.data.write().unwrap();
            *data = baseline.clone().unwrap_or_default();
        }
        self.state.bump_generation();
    }

    /// Where the loop stands: tracking, a write pending, or the last
    /// flash failed.
    pub fn status(&self) -> ServoStatus {
        self.state.status.read().unwrap().clone()
    }

    /// Watch channel that ticks on every observable change — setpoints,
    /// absorbed measurements, status. Same contract as a scenery's
    /// `subscribe()`.
    pub fn subscribe(&self) -> watch::Receiver<Generation> {
        self.state.generation_tx.subscribe()
    }

    /// Actuate: freeze the error signal into an immutable
    /// [`ChangeFlash`] and emit it through the Dio's optimistic write
    /// path. Only the changed fields travel. Returns `Ok(None)` when
    /// the error is zero — nothing dirty, nothing fired.
    ///
    /// The diff is taken synchronously at the moment of the call; the
    /// emitted flash never changes afterwards, no matter how long
    /// persistence takes or what arrives upstream meanwhile. The servo
    /// itself settles clean immediately (the staged value becomes the
    /// new baseline) and reports [`Pending`](ServoStatus::Pending) until
    /// the write-through confirms.
    ///
    /// On a servo without a baseline (opened via
    /// [`Dio::servo_new`](crate::Dio::servo_new), or on a row that
    /// vanished) the flash is an insert of the full record; the id comes
    /// from the servo's binding or the record's id column.
    pub async fn flash(&self) -> Result<Option<ChangeFlash>> {
        // Freeze synchronously: everything the flash carries is decided
        // before the first await point.
        let flash = {
            let baseline = self.state.baseline.read().unwrap();
            let data = self.state.data.read().unwrap();
            match baseline.as_ref() {
                Some(base) => {
                    let error = tracking::error_of(Some(base), &data);
                    if error.is_empty() {
                        return Ok(None);
                    }
                    let id = self
                        .state
                        .id
                        .read()
                        .unwrap()
                        .clone()
                        .expect("a servo with a baseline is bound to an id");
                    ChangeFlash::new(FlashKind::Patch, Some(id), error).with_before(base.clone())
                }
                None => {
                    if data.is_empty() {
                        return Ok(None);
                    }
                    let id = match self.state.id.read().unwrap().clone() {
                        Some(id) => id,
                        None => self.id_from_record(&data)?,
                    };
                    ChangeFlash::insert(id, data.clone())
                }
            }
        };

        // Settle clean-and-pending locally: the emitted state is the new
        // baseline. The optimistic stage makes the cache agree in a moment.
        let after = flash.after().expect("insert/patch always has an after");
        {
            *self.state.id.write().unwrap() = flash.id().map(str::to_string);
            *self.state.baseline.write().unwrap() = Some(after.clone());
            *self.state.data.write().unwrap() = after;
            *self.state.status.write().unwrap() = ServoStatus::Pending;
        }
        self.state.bump_generation();

        match self.dio.flash(flash.clone()).await {
            Ok(()) => {
                self.state.set_status(ServoStatus::Tracking);
                Ok(Some(flash))
            }
            Err(e) => {
                // The rollback restored the cache pre-image; the bus task
                // absorbs it. Report the failure — never silently.
                self.state.set_status(ServoStatus::Failed(e.to_string()));
                Err(e)
            }
        }
    }

    /// Emit a delete flash for the bound record, carrying the baseline
    /// as its pre-image.
    pub async fn delete(&self) -> Result<ChangeFlash> {
        let (id, before) = {
            let id =
                self.state.id.read().unwrap().clone().ok_or_else(|| {
                    vantage_core::error!("an unsaved servo has no record to delete")
                })?;
            (id, self.state.baseline.read().unwrap().clone())
        };
        let mut flash = ChangeFlash::delete(id);
        if let Some(b) = before {
            flash = flash.with_before(b);
        }
        self.dio.flash(flash.clone()).await?;
        {
            *self.state.baseline.write().unwrap() = None;
            self.state.data.write().unwrap().clear();
        }
        self.state.bump_generation();
        Ok(flash)
    }

    /// Feed a measurement into the loop directly. Normally the bus task
    /// does this; [`Dio::servo`](crate::Dio::servo) uses it for the
    /// initial cache seed.
    pub(crate) fn absorb(&self, incoming: Option<Record<CborValue>>) {
        self.state.absorb(incoming);
    }

    /// Resolve an insert id from the record's id column.
    fn id_from_record(&self, data: &Record<CborValue>) -> Result<String> {
        let id_column = self
            .dio
            .master()
            .get_id_column()
            .unwrap_or("id")
            .to_string();
        let id = data
            .get(&id_column)
            .map(cbor_scalar_string)
            .filter(|s| !s.is_empty());
        id.ok_or_else(|| {
            vantage_core::error!(
                "flashing a new record requires its id field",
                id_column = id_column
            )
        })
    }
}

/// The bus-tracking loop: every event about the bound record feeds the
/// measurement side of the loop from the cache.
async fn track_loop(
    state: Arc<ServoState>,
    dio_weak: Weak<DioInner>,
    mut bus: broadcast::Receiver<DioEvent>,
) {
    loop {
        if dio_weak.upgrade().is_none() {
            return;
        }
        let bound = |id: &str| state.id.read().unwrap().as_deref() == Some(id);
        match bus.recv().await {
            Ok(DioEvent::RecordChanged { id })
            | Ok(DioEvent::RecordInserted { id })
            | Ok(DioEvent::RecordRemoved { id })
                if bound(&id) =>
            {
                absorb_from_cache(&state, &dio_weak).await;
            }
            Ok(DioEvent::DatasetChanged) => {
                absorb_from_cache(&state, &dio_weak).await;
            }
            Ok(DioEvent::WritePending { id, kind }) if bound(&id) => {
                // Same filter as the revert arm: someone else's staged
                // delete is not this servo's write in flight.
                if matches!(
                    kind,
                    crate::FlashKind::Patch | crate::FlashKind::Replace | crate::FlashKind::Insert
                ) {
                    state.set_status(ServoStatus::Pending);
                }
            }
            Ok(DioEvent::WriteReverted { id, error, kind }) if bound(&id) => {
                // Only editing kinds are this servo's failure — a reverted
                // Delete/Clear belongs to its issuer (the confirm dialog),
                // and a form displaying the record must not adopt it as a
                // save failure. The restored pre-image is absorbed either way.
                if matches!(
                    kind,
                    crate::FlashKind::Patch | crate::FlashKind::Replace | crate::FlashKind::Insert
                ) {
                    state.set_status(ServoStatus::Failed(error));
                }
                absorb_from_cache(&state, &dio_weak).await;
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Missed events: re-measure from the cache.
                absorb_from_cache(&state, &dio_weak).await;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn absorb_from_cache(state: &Arc<ServoState>, dio_weak: &Weak<DioInner>) {
    let Some(inner) = dio_weak.upgrade() else {
        return;
    };
    let Some(id) = state.id.read().unwrap().clone() else {
        return;
    };
    match inner.cache.get_value(&id).await {
        Ok(value) => state.absorb(value),
        Err(e) => tracing::error!(error = %e, "servo measurement read failed"),
    }
}

/// Internal constructor — wires the bus task and returns the servo.
/// Used by [`Dio::servo`](crate::Dio::servo) and
/// [`Dio::servo_new`](crate::Dio::servo_new).
pub(crate) fn spawn_servo(dio: &Dio, id: Option<String>) -> Servo {
    let (generation_tx, _rx) = watch::channel(Generation::default());
    let state = Arc::new(ServoState {
        _tally: crate::stats::Tally::servo(),
        id: RwLock::new(id),
        baseline: RwLock::new(None),
        data: RwLock::new(Record::new()),
        status: RwLock::new(ServoStatus::Tracking),
        generation: AtomicU64::new(0),
        generation_tx,
    });

    let bus_rx = dio.inner.event_bus.subscribe();
    let dio_weak = Arc::downgrade(&dio.inner);
    let task_state = state.clone();
    let task = dio
        .inner
        .lens
        .runtime
        .spawn(track_loop(task_state, dio_weak, bus_rx));

    Servo {
        dio: dio.clone(),
        state,
        _guard: ServoGuard { task },
    }
}
