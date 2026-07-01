//! Mutation effects — the "what makes the data move" half of a faker table.
//!
//! An effect drives a shared [`FakerCtx`], which owns the in-memory store (a
//! [`MockShell`]) and a broadcast sender of [`ChangeEvent`]s. `seed` fills the
//! initial dataset (before any subscriber exists); `run` is the long-lived loop
//! that pushes live deltas. New behaviours are new `FakerEffect` impls — no
//! match statement to edit.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use fake::Fake;
use tokio::sync::broadcast;
use tokio::time::{Instant, interval};
use vantage_diorama::ChangeEvent;
use vantage_types::Record;
use vantage_vista::mocks::MockShell;

use crate::FakerColumn;
use crate::value_gen::ValueGen;

/// Shared handle an effect uses to mutate the store and broadcast deltas.
///
/// The `MockShell` store is `Arc`-shared, so the clone the effect holds and the
/// clone boxed into the master `Vista` see the same rows — a live `list`/refresh
/// observes the effect's mutations, and the broadcast keeps subscribed Dios in
/// step without a full re-list.
pub struct FakerCtx {
    shell: MockShell,
    events: broadcast::Sender<ChangeEvent>,
    columns: Vec<FakerColumn>,
    id_column: String,
    values: ValueGen,
    seq: AtomicU64,
}

impl FakerCtx {
    pub fn new(
        shell: MockShell,
        events: broadcast::Sender<ChangeEvent>,
        columns: Vec<FakerColumn>,
        id_column: String,
    ) -> Self {
        Self {
            shell,
            events,
            columns,
            id_column,
            values: ValueGen::new(),
            seq: AtomicU64::new(0),
        }
    }

    fn generate(&self, id: &str) -> Record<CborValue> {
        self.values.record_for(&self.columns, &self.id_column, id)
    }

    /// Reverse-monotonic id: newest rows get the *smallest* key, so the cache's
    /// ascending key order surfaces the latest record first — the "newest on
    /// top" fifo look, with no explicit ORDER BY.
    fn next_fifo_id(&self) -> String {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        format!("{:020}", u64::MAX - seq)
    }

    fn next_seed_id(&self) -> String {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        format!("{seq:020}")
    }

    /// Seed one row directly into the store, *without* broadcasting — used
    /// before any subscriber exists (the lens seeds the cache from this snapshot).
    pub fn seed_one(&self) -> String {
        let id = self.next_seed_id();
        self.shell.set_record(&id, self.generate(&id));
        id
    }

    /// Generate a live row: store it and broadcast an `Inserted`. Returns its id.
    pub fn push(&self) -> String {
        let id = self.next_fifo_id();
        let record = self.generate(&id);
        self.shell.set_record(&id, record.clone());
        let _ = self.events.send(ChangeEvent::Inserted {
            id: id.clone(),
            new: Some(record),
        });
        id
    }

    /// Remove a row: drop it from the store and broadcast a `Deleted`.
    pub fn expire(&self, id: &str) {
        self.shell.remove_record(id);
        let _ = self
            .events
            .send(ChangeEvent::Deleted { id: id.to_string() });
    }
}

/// A per-table mutation behaviour.
#[async_trait]
pub trait FakerEffect: Send + Sync {
    /// Populate the initial dataset. Runs before any subscriber exists.
    fn seed(&self, ctx: &FakerCtx);

    /// Long-lived loop emitting live deltas. Default: no-op (static data).
    async fn run(&self, _ctx: Arc<FakerCtx>) {}

    /// Whether [`run`](Self::run) does anything — i.e. whether a forwarder task
    /// and event subscription are worth wiring up.
    fn is_live(&self) -> bool {
        false
    }
}

/// Generate `count` rows once, then never change.
pub struct StaticEffect {
    pub count: usize,
}

#[async_trait]
impl FakerEffect for StaticEffect {
    fn seed(&self, ctx: &FakerCtx) {
        for _ in 0..self.count {
            ctx.seed_one();
        }
    }
}

/// Insert one row every `interval`, newest first, and expire each row a random
/// duration in `[retention_lo, retention_hi]` after it was added.
pub struct FifoEffect {
    pub interval: Duration,
    pub retention_lo: Duration,
    pub retention_hi: Duration,
}

impl FifoEffect {
    fn random_ttl(&self) -> Duration {
        let lo = self.retention_lo.as_millis() as u64;
        let hi = (self.retention_hi.as_millis() as u64).max(lo + 1);
        Duration::from_millis((lo..hi).fake())
    }
}

#[async_trait]
impl FakerEffect for FifoEffect {
    fn seed(&self, _ctx: &FakerCtx) {
        // fifo starts empty and fills up on its own.
    }

    fn is_live(&self) -> bool {
        true
    }

    async fn run(&self, ctx: Arc<FakerCtx>) {
        let mut ticker = interval(self.interval);
        // (expire_at, id), scanned in full each tick — retention bands overlap,
        // so a later row can be due before an earlier one; a front-only pop would
        // strand it.
        let mut pending: VecDeque<(Instant, String)> = VecDeque::new();

        loop {
            ticker.tick().await;
            let id = ctx.push();
            pending.push_back((Instant::now() + self.random_ttl(), id));

            let now = Instant::now();
            let mut i = 0;
            while i < pending.len() {
                if pending[i].0 <= now {
                    let (_, expired) = pending.remove(i).expect("index in range");
                    ctx.expire(&expired);
                } else {
                    i += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

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

    fn ctx() -> (Arc<FakerCtx>, broadcast::Receiver<ChangeEvent>) {
        let shell = MockShell::new();
        let (tx, rx) = broadcast::channel(64);
        let ctx = Arc::new(FakerCtx::new(shell, tx, columns(), "id".into()));
        (ctx, rx)
    }

    #[test]
    fn static_effect_seeds_exactly_count_rows_without_events() {
        let (ctx, mut rx) = ctx();
        StaticEffect { count: 20 }.seed(&ctx);
        // seed must not broadcast (no subscribers exist at seed time).
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
        // rows landed in the shared store.
        assert_eq!(count_store(&ctx), 20);
    }

    #[tokio::test]
    async fn push_stores_and_broadcasts_inserted() {
        let (ctx, mut rx) = ctx();
        let id = ctx.push();
        match rx.try_recv().unwrap() {
            ChangeEvent::Inserted { id: got, new } => {
                assert_eq!(got, id);
                assert!(new.is_some());
            }
            other => panic!("expected Inserted, got {other:?}"),
        }
        assert_eq!(count_store(&ctx), 1);
    }

    #[tokio::test]
    async fn expire_removes_and_broadcasts_deleted() {
        let (ctx, mut rx) = ctx();
        let id = ctx.push();
        let _ = rx.try_recv(); // drop the Inserted
        ctx.expire(&id);
        assert!(matches!(rx.try_recv().unwrap(), ChangeEvent::Deleted { id: got } if got == id));
        assert_eq!(count_store(&ctx), 0);
    }

    #[test]
    fn fifo_ids_descend_so_ascending_order_is_newest_first() {
        let (ctx, _rx) = ctx();
        let a = ctx.push();
        let b = ctx.push();
        // later insert → smaller key, so a > b lexicographically.
        assert!(a > b, "expected newer id {b} to sort before older {a}");
    }

    // The shared store's size — read straight off the shell (same module, so the
    // private field is reachable), no async round-trip through a Vista needed.
    fn count_store(ctx: &FakerCtx) -> usize {
        ctx.shell.len()
    }
}
