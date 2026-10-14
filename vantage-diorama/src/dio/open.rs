//! Opening views onto a Dio: sceneries, servos and the facade Vista.

use vantage_core::Result;
use vantage_vista::Vista;

use super::{Dio, DioShell};
use crate::scenery::{RecordSceneryBuilder, TableSceneryBuilder, ValueSceneryBuilder};
use crate::servo::{IdStrategy, Servo, spawn_servo};

impl Dio {
    /// Start a [`TableScenery`](crate::TableScenery) builder
    /// for this Dio. Chainable; call `.open().await` to spawn the
    /// reactive view.
    pub fn table_scenery(&self) -> TableSceneryBuilder {
        TableSceneryBuilder::new(self.inner.clone())
    }

    /// Start a [`RecordScenery`](crate::RecordScenery) builder for the
    /// record `id`; call `.open().await` to spawn the reactive view.
    ///
    /// By default the view opens on the cached row, `Fresh` (or
    /// `Incomplete` while an augmented row waits for its detail pass), and a
    /// cache miss, or a row outside the Dio's [`residual`](Self::residual),
    /// is `NotFound`. Chain [`load_on_miss`](RecordSceneryBuilder::load_on_miss)
    /// to fetch a missing row from the master,
    /// [`seed`](RecordSceneryBuilder::seed) to start from a row in hand, or
    /// [`sugar`](RecordSceneryBuilder::sugar) to read computed columns.
    pub fn record_scenery(&self, id: impl Into<String>) -> RecordSceneryBuilder {
        RecordSceneryBuilder::new(self.inner.clone(), id.into())
    }

    /// Open a [`Servo`] — the editing companion — for the record at `id`.
    ///
    /// The servo's baseline seeds from the cache, hydrated when the residual
    /// names an augmented value (a missing row, or one
    /// outside the Dio's [`residual`](Self::residual), starts empty and
    /// becomes an insert on the first [`flash`](crate::servo::Servo::flash))
    /// and then tracks the record live: untouched fields follow upstream
    /// changes and stay clean, touched fields lock and hold. The servo holds
    /// a strong handle to this Dio, keeping the write pipeline alive while a
    /// form is open.
    pub async fn servo(&self, id: impl Into<String>) -> Result<Servo> {
        let id = id.into();
        let servo = spawn_servo(self, Some(id.clone()), IdStrategy::FromRecord);
        if let Some(initial) = self
            .inner
            .read_admitted(self.inner.cache.as_ref(), &id)
            .await?
        {
            servo.absorb(Some(initial));
        }
        Ok(servo)
    }

    /// Open a [`Servo`] for a record that does not exist yet; the first
    /// [`flash`](crate::servo::Servo::flash) emits an insert. Identity
    /// follows `strategy`: minted UUID (v7) at creation, backend-assigned
    /// on first save, or commanded through the record's id column.
    pub fn servo_new(&self, strategy: IdStrategy) -> Servo {
        spawn_servo(self, None, strategy)
    }

    /// Start a [`ValueScenery`](crate::scenery::ValueScenery) builder.
    /// Chain `.count()` / `.sum(col)` / `.custom(closure)` /
    /// `.aggregate(...)`, then `.open().await`.
    pub fn value_scenery(&self) -> ValueSceneryBuilder {
        ValueSceneryBuilder::new(self.inner.clone())
    }

    /// The read-through facade: a fresh [`Vista`] over this Dio's cache.
    /// Each call returns an independent Vista, so callers can narrow it with
    /// conditions (any operator), a search and an order without affecting
    /// other consumers.
    ///
    /// The facade's schema mirrors `master`, every column orderable. A
    /// narrowed read goes to the master when it takes a clause and to the
    /// cache otherwise, and the facade evaluates whatever the master didn't
    /// take. Every read keeps only the rows the Dio's residual admits.
    /// Writes route through the Dio's queue.
    ///
    /// The order of a read depends on where it went. A cache read sorts with
    /// vista's evaluator ([`VistaQuery::cmp_rows`](vantage_vista::VistaQuery::cmp_rows)),
    /// which compares text by bytes; a read the master answers keeps the
    /// master's order, in the master's collation. The two can disagree: under
    /// SQL's case-insensitive order `apple` comes before `Banana`, and a
    /// cache read puts `Banana` first. An order alone reads the cache, so a
    /// handle that only sorts always gets the evaluator's order.
    pub fn cached(&self) -> Vista {
        let name = self.master().name().to_string();
        Vista::new(name, DioShell::new(self.inner.clone()))
    }
}
