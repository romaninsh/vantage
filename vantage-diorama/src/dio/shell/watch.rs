//! Live subscription for a Dio exposed as another Dio's master.
//!
//! The stream is fed by the Dio's own event bus. Every id-bearing event is
//! re-evaluated through the handle's narrowing and the Dio's residual against
//! the Dio's cache, so the stream speaks in terms of *this handle's* set: a
//! row patched into the filter arrives as `Inserted`, one patched out of it as
//! `Deleted`. The consumer's `Dio::apply_change` upserts whatever it is given
//! without checking conditions, so the filtering has to happen here.

use std::collections::HashSet;
use std::sync::{Arc, Weak};

use futures::stream;
use tokio::sync::broadcast::{self, error::RecvError};
use vantage_core::Result;
use vantage_vista::{VistaChange, VistaChangeStream, VistaMetadata, VistaQuery};

use super::DioShell;
use crate::dio::{DioEvent, DioInner};

impl DioShell {
    pub(crate) async fn follow(&self) -> Result<VistaChangeStream> {
        // Subscribe before listing, so a change landing in between is replayed
        // rather than lost.
        let events = self.dio.event_bus.subscribe();
        let mut follow = Follow {
            events,
            // Weak: a parked subscription must not keep the parent Dio alive.
            dio: Arc::downgrade(&self.dio),
            query: self.query.clone(),
            metadata: self.metadata.clone(),
            members: HashSet::new(),
        };
        follow.relist(self.dio.clone()).await?;
        Ok(Box::pin(stream::unfold(follow, |mut follow| async move {
            let item = follow.next().await?;
            Some((item, follow))
        })))
    }
}

struct Follow {
    events: broadcast::Receiver<DioEvent>,
    dio: Weak<DioInner>,
    query: VistaQuery,
    /// The facade's schema, which tells a flat cell from a dotted path.
    metadata: VistaMetadata,
    /// Ids currently in the narrowed set, as the consumer last saw it. Tells a
    /// row entering the set (`Inserted`) from one already in it (`Updated`),
    /// and which rows leaving the filter need a `Deleted`.
    members: HashSet<String>,
}

impl Follow {
    /// The next change for the consumer, or `None` once the Dio is gone.
    async fn next(&mut self) -> Option<Result<VistaChange>> {
        loop {
            let event = match self.events.recv().await {
                Ok(event) => Some(event),
                Err(RecvError::Closed) => return None,
                // Events were dropped: re-list rather than end the stream.
                Err(RecvError::Lagged(_)) => None,
            };
            let dio = self.dio.upgrade()?;
            let change = match event {
                Some(
                    DioEvent::RecordInserted { id }
                    | DioEvent::RecordChanged { id }
                    | DioEvent::RecordRemoved { id }
                    | DioEvent::WritePending { id, .. }
                    | DioEvent::WriteReverted { id, .. },
                ) => self.reconcile(dio, id).await.transpose(),
                None | Some(DioEvent::DatasetChanged | DioEvent::Seeded) => {
                    Some(self.relist(dio).await.map(|()| VistaChange::Invalidated))
                }
                Some(_) => None,
            };
            if change.is_some() {
                return change;
            }
        }
    }

    /// Re-evaluate one row: its current cached value against the handle's
    /// query and the Dio's residual, hydrated first when that needs its
    /// augmented values.
    async fn reconcile(&mut self, dio: Arc<DioInner>, id: String) -> Result<Option<VistaChange>> {
        let row = self.reader(dio).cached_in_view(&id, false).await?;
        Ok(match row {
            Some(value) if self.members.insert(id.clone()) => {
                Some(VistaChange::Inserted { id, value })
            }
            Some(value) => Some(VistaChange::Updated { id, value }),
            None if self.members.remove(&id) => Some(VistaChange::Deleted { id }),
            None => None,
        })
    }

    /// Rebuild membership from the same read the consumer's own listing uses.
    async fn relist(&mut self, dio: Arc<DioInner>) -> Result<()> {
        let rows = self.reader(dio).read().await?;
        self.members = rows.into_keys().collect();
        Ok(())
    }

    /// A read-only handle over `dio` with this follow's query.
    fn reader(&self, dio: Arc<DioInner>) -> DioShell {
        DioShell::reader(dio, self.query.clone(), self.metadata.clone())
    }
}
