//! [`MemoryTx`]: one store's share of a [`Transaction`].
//!
//! On first use it claims the store, so another transaction on the same
//! store waits at its own first use until this one finishes: blocks
//! serialize, and a read-modify-write in one never loses another's update.
//! The wait is bounded ([`MemoryStore::with_transaction_wait`]): two blocks
//! taking two stores in opposite orders fail one of them, as a database
//! reports a deadlock, instead of waiting on each other for good.
//!
//! Each table it touches is copied then, and every read and write of the
//! transaction goes to the copy, which records its writes. Commit replays
//! them onto the store's tables as they are by then, so writes made outside
//! the transaction meanwhile stay; an insert whose id was taken outside
//! meanwhile fails the commit with nothing applied. Rollback (or a drop)
//! discards the copies. Callers outside a transaction never wait on the
//! claim and never see a copy.
//!
//! Unlike SQL, which fails a commit only for database errors, any failed
//! operation inside a transaction fails its commit here, including a
//! Vista-level refusal the caller caught.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::time::timeout;
use vantage_core::{Result, VantageError, error};
use vantage_vista::{Transaction, TxKey, TxParticipant, WeakTransaction};

use crate::{MemoryStore, MemoryTable, MemoryTableHandle};

const FAILED_STATEMENT: &str = "a statement failed inside the transaction; rolled back";

pub(crate) struct MemoryTx {
    claim: Arc<Mutex<()>>,
    wait: Duration,
    /// Taken while claiming, so the transaction's concurrent first calls
    /// claim once. Never held with `open`, which commit and rollback need.
    claiming: Mutex<()>,
    open: Mutex<Open>,
    failed: AtomicBool,
    /// Set under the `open` lock by commit or rollback.
    closed: AtomicBool,
    // Weak: the transaction holds this participant.
    owner: WeakTransaction,
}

#[derive(Default)]
struct Open {
    claimed: Option<OwnedMutexGuard<()>>,
    copies: Vec<Copy>,
}

/// A table the transaction touched: the store's, and the copy.
struct Copy {
    live: MemoryTableHandle,
    work: MemoryTableHandle,
}

impl Open {
    fn copy_of(&mut self, live: &MemoryTableHandle) -> MemoryTableHandle {
        if let Some(copy) = self.copies.iter().find(|c| Arc::ptr_eq(&c.live, live)) {
            return copy.work.clone();
        }
        let work = Arc::new(live.fork());
        self.copies.push(Copy {
            live: live.clone(),
            work: work.clone(),
        });
        work
    }
}

impl MemoryTx {
    pub(crate) fn key(store: &MemoryStore) -> TxKey {
        TxKey::of(store.claim())
    }

    pub(crate) fn new(store: &MemoryStore, owner: &Transaction) -> Self {
        Self {
            claim: store.claim().clone(),
            wait: store.transaction_wait(),
            claiming: Mutex::default(),
            open: Mutex::default(),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            owner: owner.downgrade(),
        }
    }

    /// Runs `op` on the transaction's copy of `live`, claiming the store
    /// and copying the table first if this is their first use. An `Err`
    /// from `op`, or a claim that timed out, fails the commit.
    pub(crate) async fn run<R>(
        &self,
        live: &MemoryTableHandle,
        op: impl FnOnce(&MemoryTableHandle) -> Result<R> + Send,
    ) -> Result<R> {
        match self.owner.upgrade() {
            Some(tx) if !tx.is_finished() => {}
            Some(_) => return Err(finished()),
            None => {
                // Dropped unfinished: release the store now rather than
                // when the last handle goes.
                if let Ok(mut open) = self.open.try_lock() {
                    *open = Open::default();
                }
                return Err(finished());
            }
        }
        self.claimed().await?;
        let mut open = self.open.lock().await;
        // Commit or rollback may have run since the checks above.
        if self.closed.load(Ordering::SeqCst) {
            return Err(finished());
        }
        let work = open.copy_of(live);
        let out = op(&work);
        if out.is_err() {
            self.failed.store(true, Ordering::SeqCst);
        }
        out
    }

    /// Waits, up to the store's transaction wait, until this transaction
    /// holds the store.
    async fn claimed(&self) -> Result<()> {
        let _claiming = self.claiming.lock().await;
        if self.open.lock().await.claimed.is_some() {
            return Ok(());
        }
        let Ok(guard) = timeout(self.wait, self.claim.clone().lock_owned()).await else {
            self.failed.store(true, Ordering::SeqCst);
            return Err(error!(
                "the memory store is busy with another transaction",
                waited = format!("{:?}", self.wait)
            ));
        };
        let mut open = self.open.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Err(finished());
        }
        open.claimed = Some(guard);
        Ok(())
    }

    /// The copies and the claim, taken out for good: later calls are refused.
    async fn close(&self) -> Open {
        let mut open = self.open.lock().await;
        self.closed.store(true, Ordering::SeqCst);
        std::mem::take(&mut *open)
    }
}

fn finished() -> VantageError {
    error!("transaction finished")
}

#[async_trait]
impl TxParticipant for MemoryTx {
    /// Every touched table's write lock is held while the copies are
    /// replayed, so a reader outside sees all of the block's writes or
    /// none. The claim is released only afterwards, so the next transaction
    /// copies the committed rows.
    async fn commit(&self) -> Result<()> {
        let open = self.close().await;
        if self.failed.load(Ordering::SeqCst) {
            return Err(error!(FAILED_STATEMENT));
        }
        let pairs: Vec<_> = open
            .copies
            .iter()
            .map(|copy| (&*copy.live, &*copy.work))
            .collect();
        let applied = MemoryTable::apply_all(&pairs);
        drop(open);
        applied
    }

    async fn rollback(&self) -> Result<()> {
        self.close().await;
        Ok(())
    }
}
