//! Synthetic, optionally-live datasource for Vantage.
//!
//! A faker table generates realistic rows and — for live effects — keeps
//! mutating them over time, pushing genuine [`ChangeEvent`]s so a subscribed
//! Dio animates inserts/expiries instead of re-listing. It's for testing and
//! demos: exercise the reactive Diorama pipeline without standing up a backend.
//!
//! Two halves:
//! - [`ValueGen`] decides *what* a cell contains (name-aware, then type fallback).
//! - a [`FakerEffect`] decides *how the data moves* — [`StaticEffect`] (generate
//!   once) or [`FifoEffect`] (insert newest-first, expire after a while).
//!
//! [`FakerTable::build`] ties them together: it seeds a shared [`MockShell`]
//! store, wraps it in a [`Vista`], and (for live effects) spawns the mutation
//! loop. The returned handle exposes both the [`Vista`] to list from and a
//! broadcast [`Sender`](broadcast::Sender) to subscribe to for live deltas.

pub mod effect;
pub mod live_folder;
pub mod pulse;
pub mod value_gen;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use vantage_diorama::ChangeEvent;
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

pub use effect::{FakerCtx, FakerEffect, FifoEffect, StaticEffect};
pub use live_folder::{
    EVENT_TYPES, Entry, EntryKind, LiveFolderConfig, LiveFolderSim, PushMode, format_ts,
};
pub use pulse::{PulseConfig, PulseKey, PulseRole, PulseSim};
pub use value_gen::ValueGen;

/// One column of a faker table: a name, a declared type, and free-form flags
/// (e.g. `"id"`). [`ValueGen`] uses `name` first, then `ty`, to pick a value.
#[derive(Clone, Debug)]
pub struct FakerColumn {
    pub name: String,
    pub ty: String,
    pub flags: Vec<String>,
}

/// A materialized faker table: a [`Vista`] to read from, the broadcast
/// [`broadcast::Sender`] carrying live deltas, and the handle of the effect's
/// mutation loop (if any).
///
/// Drop the table to stop the loop — the [`JoinHandle`] is aborted with the task
/// when the last reference goes away. Keep it alive for as long as the Vista is
/// in use.
pub struct FakerTable {
    /// Reads the shared store — hand this to the Diorama lens.
    pub vista: Vista,
    /// Subscribe with [`Sender::subscribe`](broadcast::Sender::subscribe) to
    /// receive [`ChangeEvent`]s and forward them into a Dio.
    pub events: broadcast::Sender<ChangeEvent>,
    /// The live mutation loop, `None` for static effects. Held only for its
    /// abort-on-drop guard — dropping the table stops the loop.
    _task: Option<AbortOnDrop>,
}

impl FakerTable {
    /// Build a faker table from its schema and a chosen effect.
    ///
    /// Seeds the store synchronously (before any subscriber exists, so seed rows
    /// are not broadcast), then, if the effect [`is_live`](FakerEffect::is_live),
    /// spawns its [`run`](FakerEffect::run) loop on the current Tokio runtime.
    pub fn build(
        name: impl Into<String>,
        columns: Vec<FakerColumn>,
        id_column: impl Into<String>,
        effect: Box<dyn FakerEffect>,
    ) -> Self {
        let shell = MockShell::new();
        let (events, _) = broadcast::channel(EVENT_CAPACITY);

        let ctx = std::sync::Arc::new(FakerCtx::new(
            shell.clone(),
            events.clone(),
            columns,
            id_column.into(),
        ));

        effect.seed(&ctx);

        let vista = Vista::new(name, Box::new(shell));

        let task = effect.is_live().then(|| {
            AbortOnDrop(tokio::spawn(async move {
                effect.run(ctx).await;
            }))
        });

        Self {
            vista,
            events,
            _task: task,
        }
    }

    /// Split into the master [`Vista`] and a live [`FakerHandle`].
    ///
    /// `make_dio` consumes the Vista by value, so hand it the returned Vista and
    /// keep the [`FakerHandle`] alive — the handle owns the mutation loop (drop
    /// it and the loop stops) and exposes the broadcast [`broadcast::Sender`] to subscribe a
    /// forwarder that feeds `dio.handle_event`.
    pub fn split(self) -> (Vista, FakerHandle) {
        let Self {
            vista,
            events,
            _task,
        } = self;
        (vista, FakerHandle { events, _task })
    }
}

/// The live half of a [`FakerTable`] once its [`Vista`](FakerTable::split) has
/// been handed to a Dio: the delta [`Sender`](broadcast::Sender) and the
/// abort-on-drop mutation-loop guard.
pub struct FakerHandle {
    /// Subscribe to receive [`ChangeEvent`]s and forward them into a Dio.
    pub events: broadcast::Sender<ChangeEvent>,
    /// Held only for its abort-on-drop guard — dropping the handle stops the loop.
    _task: Option<AbortOnDrop>,
}

/// Broadcast backlog for a lagged subscriber. A subscriber that falls this far
/// behind gets a `Lagged` error and should resync via a Vista `list`; the store
/// is the source of truth, so no delta is truly lost.
const EVENT_CAPACITY: usize = 1024;

/// Aborts its task when dropped, so a dropped [`FakerTable`] stops mutating.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use vantage_dataset::prelude::ReadableValueSet;

    fn columns() -> Vec<FakerColumn> {
        vec![
            FakerColumn {
                name: "id".into(),
                ty: "string".into(),
                flags: vec!["id".into()],
            },
            FakerColumn {
                name: "email".into(),
                ty: "string".into(),
                flags: vec![],
            },
        ]
    }

    #[tokio::test]
    async fn static_table_seeds_rows_and_has_no_live_task() {
        let table = FakerTable::build(
            "events",
            columns(),
            "id",
            Box::new(StaticEffect { count: 20 }),
        );
        let rows = table.vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 20);
    }

    #[tokio::test]
    async fn fifo_table_broadcasts_inserts_as_it_runs() {
        let table = FakerTable::build(
            "events",
            columns(),
            "id",
            Box::new(FifoEffect {
                interval: Duration::from_millis(5),
                retention_lo: Duration::from_secs(30),
                retention_hi: Duration::from_secs(60),
            }),
        );

        let mut rx = table.events.subscribe();
        let got_insert = tokio::time::timeout(Duration::from_secs(1), recv_insert(&mut rx))
            .await
            .expect("expected an Inserted within 1s");
        assert!(got_insert);

        // Dropping the table aborts the loop, so a live table stays scoped.
        drop(table);
    }

    async fn recv_insert(rx: &mut broadcast::Receiver<ChangeEvent>) -> bool {
        loop {
            match rx.recv().await {
                Ok(ChangeEvent::Inserted { .. }) => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
    }
}
