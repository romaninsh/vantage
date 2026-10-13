//! Opening views onto a Dio: sceneries, servos and the facade Vista.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::Vista;

use super::{Dio, DioShell};
use crate::scenery::record::spawn_record_scenery;
use crate::scenery::{RecordScenery, RecordStatus, TableSceneryBuilder, ValueSceneryBuilder};
use crate::servo::{IdStrategy, Servo, spawn_servo};

impl Dio {
    /// Start a [`TableScenery`](crate::TableScenery) builder
    /// for this Dio. Chainable; call `.open().await` to spawn the
    /// reactive view.
    pub fn table_scenery(&self) -> TableSceneryBuilder {
        TableSceneryBuilder::new(self.inner.clone())
    }

    /// Open a reactive view onto a single record by id. Reads the
    /// cache once at creation:
    ///
    /// - cache hit → `RecordStatus::Fresh`, record exposed
    /// - cache miss, or a row outside the Dio's [`residual`](Self::residual)
    ///   → `RecordStatus::NotFound`, record = `None`
    ///
    /// A residual naming an augmented value judges the row hydrated: the
    /// read blocks on its detail pass. No master fetch on miss (the cache is
    /// the source of truth). Use [`Dio::apply_change`](Self::apply_change)
    /// to seed the row.
    pub async fn record_scenery(&self, id: impl Into<String>) -> Result<Arc<dyn RecordScenery>> {
        self.inner.refresh_if_stale();
        let id = id.into();
        let cached = self.inner.read_admitted(self.inner.cache.as_ref(), &id);
        let (initial_record, initial_status) = match cached.await? {
            Some(rec) => (Some(rec), RecordStatus::Fresh),
            None => (None, RecordStatus::NotFound),
        };
        Ok(spawn_record_scenery(
            &self.inner,
            id,
            initial_record,
            initial_status,
            None,
        ))
    }

    /// [`record_scenery`](Self::record_scenery) with `sugar`'s outputs
    /// merged into the row it reads; nothing reaches the cache.
    pub async fn record_scenery_sugared(
        &self,
        id: impl Into<String>,
        sugar: crate::Sugar,
    ) -> Result<Arc<dyn RecordScenery>> {
        self.inner.refresh_if_stale();
        let reader = crate::scenery::sugar::SugaredCache::wrap(self.inner.cache.clone(), sugar);
        let id = id.into();
        let (initial_record, initial_status) =
            match self.inner.read_admitted(reader.as_ref(), &id).await? {
                Some(rec) => (Some(rec), RecordStatus::Fresh),
                None => (None, RecordStatus::NotFound),
            };
        Ok(spawn_record_scenery(
            &self.inner,
            id,
            initial_record,
            initial_status,
            Some(reader),
        ))
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

    /// Open a reactive view onto a single record with the row already
    /// in hand — the parent grid hands its current row off to the
    /// detail view without a cache round-trip. Status is `Fresh`.
    pub fn record_scenery_with(
        &self,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Arc<dyn RecordScenery> {
        self.inner.refresh_if_stale();
        spawn_record_scenery(
            &self.inner,
            id.into(),
            Some(record),
            RecordStatus::Fresh,
            None,
        )
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
    pub fn cached(&self) -> Vista {
        let name = self.master().name().to_string();
        Vista::new(name, DioShell::new(self.inner.clone()))
    }
}
