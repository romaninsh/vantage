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
//! The servo is a **change draft**: the baseline moves only on measured
//! upstream state, never on hope. [`flash`](Servo::flash) freezes the
//! error signal at fire time into an immutable
//! [`ChangeFlash`](crate::ChangeFlash) carrying only the changed fields and
//! writes it through [`Dio::flash`] — but the setpoints stay locked until
//! the write resolves. Confirmation absorbs the confirmed record and
//! convergence releases every lock; rejection absorbs the restored
//! pre-image and the setpoints still stand — the user's draft survives a
//! failed save, reported through [`ServoStatus::Failed`] as a
//! [`FlashRejection`] (with per-field errors when the write path named
//! them).
//!
//! Identity is the servo's, not the form's: [`Dio::servo_new`] mints a
//! time-ordered UUID up front (or defers to the backend with
//! [`IdStrategy::Auto`]), so a retried create reuses the same id — if
//! the first insert actually landed, the retry patches over it as a
//! noop instead of duplicating the record.
//!
//! A servo holds a **strong** Dio handle — deliberately, unlike
//! sceneries: while a form is open, the write pipeline it will flash
//! through must stay alive.

mod delete;
mod draft;
mod flash;
mod tracking;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use ciborium::Value as CborValue;
use tokio::sync::watch;
use vantage_types::Record;

use crate::dio::{Dio, DioInner, Generation};
use crate::ops::FlashRejection;

/// Where the servo loop currently stands.
#[derive(Debug, Clone)]
pub enum ServoStatus {
    /// Following the measurement; no write in flight.
    Tracking,
    /// A flash was emitted and its write-through hasn't confirmed yet.
    Pending,
    /// The last flash was rejected and rolled back. The setpoints are
    /// still held — the draft survives; the rejection carries per-field
    /// errors when the write path named them.
    Failed(FlashRejection),
}

/// How an unsaved servo ([`Dio::servo_new`]) gets its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdStrategy {
    /// Mint a time-ordered UUID (v7) the moment the servo opens and
    /// bind the servo to it — every save reuses it, so a retried create
    /// can't duplicate the record. Identity is the binding's, not the
    /// draft's: the id column joins the insert record at flash time, an
    /// untouched form stays clean, and saving it fires nothing.
    #[default]
    Uuid,
    /// The backend assigns the id on the first save
    /// ([`ChangeFlash::insert_new`](crate::ChangeFlash::insert_new), through
    /// the Lens writer when there is one); the servo binds to the created
    /// row.
    Auto,
    /// The id comes from the record's id column at flash time — the
    /// caller commands it like any other field.
    FromRecord,
}

pub(crate) struct ServoState {
    /// Live-instance census (see [`crate::stats`]).
    _tally: crate::stats::Tally,
    id: RwLock<Option<String>>,
    strategy: IdStrategy,
    baseline: RwLock<Option<Record<CborValue>>>,
    data: RwLock<Record<CborValue>>,
    status: RwLock<ServoStatus>,
    /// Non-zero while this servo's own flash is in flight. The bus task
    /// holds absorbs off for the window so the optimistic stage of our
    /// own write can't masquerade as an upstream measurement and release
    /// the locks before the write actually resolves.
    in_flight: AtomicU64,
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
    /// Weak, deliberately — `Servo` itself already holds the strong `Dio`
    /// that keeps the write pipeline alive while the form is open. The
    /// guard must not extend that lifetime further; it only reaches back
    /// in to decrement the census and emit the closing line.
    dio_weak: Weak<DioInner>,
}

impl Drop for ServoGuard {
    fn drop(&mut self) {
        self.task.abort();
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.servo_census.fetch_sub(1, Ordering::Relaxed);
            dio.emit_census("servo", "closed");
        }
    }
}

impl Servo {
    /// The record id this servo is bound to. Bound at creation for
    /// [`Dio::servo`](crate::Dio::servo) and [`IdStrategy::Uuid`];
    /// `None` until the first confirmed save for [`IdStrategy::Auto`].
    pub fn id(&self) -> Option<String> {
        self.state.id.read().unwrap().clone()
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

    /// Feed a measurement into the loop directly. Normally the bus task
    /// does this; [`Dio::servo`](crate::Dio::servo) uses it for the
    /// initial cache seed.
    pub(crate) fn absorb(&self, incoming: Option<Record<CborValue>>) {
        self.state.absorb(incoming);
    }
}

/// Wire the bus task and return the servo, bound to `id` when given. With
/// [`IdStrategy::Uuid`] and no id, identity is minted here — before the
/// first save — and bound; the insert record picks the id column up at
/// flash time.
pub(crate) fn spawn_servo(dio: &Dio, id: Option<String>, strategy: IdStrategy) -> Servo {
    let mut id = id;
    // Identity is the binding's, never the draft's: minting must not
    // dirty the data (an untouched form stays clean and fires nothing).
    if id.is_none() && strategy == IdStrategy::Uuid {
        id = Some(uuid::Uuid::now_v7().to_string());
    }

    let (generation_tx, _rx) = watch::channel(Generation::default());
    let state = Arc::new(ServoState {
        _tally: crate::stats::Tally::servo(),
        id: RwLock::new(id),
        strategy,
        baseline: RwLock::new(None),
        data: RwLock::new(Record::new()),
        status: RwLock::new(ServoStatus::Tracking),
        in_flight: AtomicU64::new(0),
        generation: AtomicU64::new(0),
        generation_tx,
    });

    let bus_rx = dio.inner.event_bus.subscribe();
    let dio_weak = Arc::downgrade(&dio.inner);
    let task = dio.inner.lens.runtime.spawn(tracking::track_loop(
        state.clone(),
        dio_weak.clone(),
        bus_rx,
    ));

    dio.inner.servo_census.fetch_add(1, Ordering::Relaxed);
    dio.inner.emit_census("servo", "opened");

    Servo {
        dio: dio.clone(),
        state,
        _guard: ServoGuard { task, dio_weak },
    }
}
