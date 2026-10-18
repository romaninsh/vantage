//! Telling the other Dios over this table what settled in this Dio's cache.
//!
//! Three rules keep two Dios from echoing facts back and forth forever:
//! a Dio never applies its own facts, applying a fact never publishes one
//! (see [`applying`]), and a fact whose row equals the cached one writes
//! nothing.

use std::future::Future;

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::DioInner;
use crate::lens::CacheStatus;
use crate::lens::hub::{HubFact, TableChange};

tokio::task_local! {
    /// Set while a Dio applies another Dio's fact. Whatever that settles in
    /// its cache (a re-read of its set included) came from the hub, so it is
    /// not sent back.
    static APPLYING: ();
}

/// Run `fut` as the application of a peer's fact: nothing it settles is
/// published.
pub(crate) async fn applying<F: Future>(fut: F) -> F::Output {
    APPLYING.scope((), fut).await
}

impl DioInner {
    /// Whether a fact published now would reach anyone: the Dio is in a
    /// hub, some other Dio listens there, and this isn't the application of
    /// a peer's fact. A Dio with no peer publishes nothing, so its bus sees
    /// exactly what it would without the hub.
    pub(crate) fn publishes(&self) -> bool {
        self.hub.as_ref().is_some_and(|hub| hub.members() > 1) && APPLYING.try_with(|_| ()).is_err()
    }

    /// Send `change` to the other Dios over this table, when [`publishes`](Self::publishes).
    pub(crate) fn publish(&self, change: TableChange) {
        if !self.publishes() {
            return;
        }
        if let Some(hub) = &self.hub {
            self.published
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            hub.send(HubFact {
                origin: self.serial,
                store: self.cache_table_name.clone(),
                change,
            });
        }
    }

    /// Publish row `id` as the cache holds it now, once a write to it is
    /// confirmed. A row the cache no longer holds publishes nothing: a
    /// delete publishes itself, and anything else that removed it (a re-read
    /// that got there first) has its own say.
    pub(crate) async fn publish_row(&self, id: &str) {
        if !self.publishes() {
            return;
        }
        match self.cache.get_value_with_status(id).await {
            Ok(Some((row, status))) => self.publish(self.upserted(id, row, status)),
            Ok(None) => {}
            Err(e) => tracing::warn!(
                target: "vantage_diorama::hub",
                table = %self.cache_table_name,
                id,
                error = %e,
                "a settled row couldn't be read back; the other Dios over the table miss it",
            ),
        }
    }

    /// The fact that row `id` now holds `row`, stored with `status`.
    pub(crate) fn upserted(
        &self,
        id: &str,
        row: Record<CborValue>,
        status: CacheStatus,
    ) -> TableChange {
        let complete = !self.has_dio_augment() || !self.awaits_hydration(id, &row, status);
        TableChange::Upserted {
            id: id.to_string(),
            row,
            complete,
        }
    }

    /// The fact for a row this Dio found gone from its set. Over the whole
    /// table that is a delete. A narrowed Dio only knows the row left *its*
    /// set, which says nothing about anyone else's, so the others re-check.
    pub(crate) fn vanished(&self, id: &str) -> TableChange {
        let id = id.to_string();
        if self.covers_less_than_the_table() {
            TableChange::Recheck { id }
        } else {
            TableChange::Deleted { id }
        }
    }

    /// Whether anything narrows this Dio's set below the whole table: the
    /// master's clauses, the residual's (which never holds a native
    /// condition), or a rule hidden in the master, which a Dio built with
    /// [`recheck_facts`](crate::DioBuilder::recheck_facts) declares.
    pub(crate) fn covers_less_than_the_table(&self) -> bool {
        self.master.is_narrowed()
            || !self.residual.conditions.is_empty()
            || self.residual.search.is_some()
            || self.recheck_facts
    }
}
