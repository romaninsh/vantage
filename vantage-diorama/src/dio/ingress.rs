//! External change entering a Dio: [`Dio::apply_change`] for one change and
//! [`Dio::reconcile`] for a full set, plus the crate's own cache-and-announce
//! helpers.

use std::collections::HashSet;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use super::pending::SettleMark;
use super::{Dio, DioEvent};
use crate::ops::ChangeEvent;

impl Dio {
    /// Apply a single upstream [`ChangeEvent`] to the cache and publish the
    /// matching bus event, in one call:
    /// - `Inserted` → cache upsert + [`DioEvent::RecordInserted`] (a new row:
    ///   sceneries re-derive their index so it appears);
    /// - `Updated` → cache upsert + [`DioEvent::RecordChanged`] (a repaint of an
    ///   existing row);
    /// - `Deleted` → cache delete + [`DioEvent::RecordRemoved`];
    /// - `Invalidated` → a full [`refresh`](Self::refresh), which re-reads the
    ///   set.
    ///
    /// A `None` value on `Inserted`/`Updated` publishes the event without
    /// touching the cache (notify-only); push sources that carry the row (the
    /// SurrealDB LIVE path) always supply it.
    pub async fn apply_change(&self, evt: ChangeEvent) -> Result<()> {
        match evt {
            ChangeEvent::Inserted { id, new } => {
                if let Some(record) = new {
                    self.inner.cache.insert_value(&id, &record).await?;
                }
                let _ = self.inner.event_bus.send(DioEvent::RecordInserted { id });
            }
            ChangeEvent::Updated { id, new } => {
                if let Some(record) = new {
                    self.inner.cache.insert_value(&id, &record).await?;
                }
                let _ = self.inner.event_bus.send(DioEvent::RecordChanged { id });
            }
            ChangeEvent::Deleted { id } => self.removed(id).await?,
            ChangeEvent::Invalidated => self.refresh().await?,
        }
        Ok(())
    }

    /// Make the cache hold exactly `snapshot`, the whole set as the master
    /// reports it, keyed by id:
    /// - every row in it that differs from the cached row is written, except
    ///   one with a flash in flight;
    /// - every cached row absent from it is deleted, except one with a flash
    ///   in flight;
    /// - the refresh time is recorded (see
    ///   [`Refresher::max_age`](crate::Refresher::max_age)), and the cache is
    ///   recorded as a whole copy;
    /// - one [`DatasetChanged`](DioEvent::DatasetChanged) is published.
    ///
    /// A row with a flash in flight is left to the flash, because the
    /// snapshot may predate the write and would revert the staged value.
    ///
    /// The snapshot is taken to be current as of this call. A caller that
    /// listed it earlier loses rows a flash settled in between; loaders and
    /// refreshers use [`LoaderCx::reconcile`](crate::LoaderCx::reconcile),
    /// which knows when their call began.
    pub async fn reconcile(&self, snapshot: IndexMap<String, Record<CborValue>>) -> Result<()> {
        let mark = self.inner.pending_flashes.mark();
        self.reconcile_since(&mark, snapshot).await
    }

    /// [`reconcile`](Self::reconcile) of a snapshot listed after `mark`: rows
    /// whose flash settled since `mark` are left alone too, written or
    /// absent, because the snapshot may predate their write.
    pub(crate) async fn reconcile_since(
        &self,
        mark: &SettleMark,
        snapshot: IndexMap<String, Record<CborValue>>,
    ) -> Result<()> {
        let pending = &self.inner.pending_flashes;
        let cache = &self.inner.cache;
        let keep: HashSet<String> = snapshot.keys().cloned().collect();
        let cached = cache.list_values().await?;
        // A row the cache already holds unchanged is not rewritten.
        let rows: IndexMap<_, _> = snapshot
            .into_iter()
            .filter(|(id, row)| !pending.guarded_since(mark, id) && cached.get(id) != Some(row))
            .collect();
        if !rows.is_empty() {
            cache.insert_values(rows).await?;
        }
        self.inner.drop_vanished(cached.keys(), &keep, mark).await?;
        self.inner.stamp_whole().await;
        self.notify_dataset_changed();
        Ok(())
    }

    /// Publish [`DioEvent::DatasetChanged`]: the set of records changed (rows
    /// appeared, vanished, or reordered), so sceneries re-read their state.
    pub(crate) fn notify_dataset_changed(&self) {
        let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
    }

    /// Publish [`DioEvent::Seeded`]: the whole set is in the cache now. Sent
    /// after an eager start, which writes the cache directly and so raises no
    /// events of its own.
    pub(crate) fn notify_seeded(&self) {
        let _ = self.inner.event_bus.send(DioEvent::Seeded);
    }

    /// Remove `id` from the cache and publish [`DioEvent::RecordRemoved`].
    /// `Ok(())` when the row wasn't cached.
    pub(crate) async fn removed(&self, id: impl Into<String>) -> Result<()> {
        let id = id.into();
        self.inner.cache.delete_value(&id).await?;
        let _ = self.inner.event_bus.send(DioEvent::RecordRemoved { id });
        Ok(())
    }
}

impl From<vantage_vista::VistaChange> for ChangeEvent {
    fn from(change: vantage_vista::VistaChange) -> Self {
        use vantage_vista::VistaChange as V;
        match change {
            V::Inserted { id, value } => ChangeEvent::Inserted {
                id,
                new: Some(value),
            },
            V::Updated { id, value } => ChangeEvent::Updated {
                id,
                new: Some(value),
            },
            V::Deleted { id } => ChangeEvent::Deleted { id },
            // A change this version doesn't know: re-read everything.
            _ => ChangeEvent::Invalidated,
        }
    }
}
