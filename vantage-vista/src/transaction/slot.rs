//! [`TxSlot`]: one datasource's share of a [`Transaction`], whatever the
//! backend. Every clone of the datasource in transaction mode holds the same
//! `TxSlot` (behind an `Arc`, as the participant), so their operations run on
//! one backend transaction, begun on first use.
//!
//! A backend implements [`TxBackend`]: how to begin (a SQL connection after
//! `BEGIN`, a MongoDB session after `start_transaction`, …), commit and roll
//! back. It joins with
//! `tx.participant(key, || TxSlot::new(backend, tx))` inside `rebind`, and runs
//! each operation as
//!
//! ```ignore
//! let mut open = slot.open().await?; // begins on first use; one op at a time
//! slot.watch(run(&mut *open)).await   // a failed or dropped op fails the commit
//! ```
//!
//! The slot owns the rest: the `failed` flag, refusing use after the
//! transaction finished (also when commit races an operation), and the
//! participant's commit (rolled back instead when an operation failed, with
//! `a statement failed inside the transaction; rolled back`) and rollback.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use tokio::sync::{Mutex, MutexGuard};
use vantage_core::{Result, VantageError, error};

use super::{Transaction, TxParticipant, WeakTransaction};

/// The commit error after an operation inside the transaction failed.
const FAILED_STATEMENT: &str = "a statement failed inside the transaction; rolled back";

/// What a [`TxSlot`] needs from a backend. Errors come back already worded
/// (`couldn't begin the transaction`, `transaction commit failed`, …).
#[async_trait]
pub trait TxBackend: Send + Sync + 'static {
    /// The begun transaction's handle: a connection, a session.
    type Open: Send + 'static;

    async fn begin(&self) -> Result<Self::Open>;
    async fn commit(&self, open: Self::Open) -> Result<()>;
    async fn rollback(&self, open: Self::Open) -> Result<()>;
}

pub struct TxSlot<B: TxBackend> {
    backend: B,
    slot: Mutex<Option<B::Open>>,
    failed: AtomicBool,
    /// Set under the slot lock by commit or rollback.
    closed: AtomicBool,
    // Weak: the transaction holds this participant, so a strong handle back
    // would keep both alive and the backend's transaction would never roll
    // back on drop.
    owner: WeakTransaction,
}

impl<B: TxBackend> fmt::Debug for TxSlot<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TxSlot")
            .field("failed", &self.failed)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl<B: TxBackend> TxSlot<B> {
    pub fn new(backend: B, owner: &Transaction) -> Self {
        Self {
            backend,
            slot: Mutex::new(None),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            owner: owner.downgrade(),
        }
    }

    /// The transaction this slot belongs to, while it exists.
    pub fn transaction(&self) -> Option<Transaction> {
        self.owner.upgrade()
    }

    /// The open transaction, begun now if nothing has run on it yet. Held
    /// until the guard drops, so operations on it run one at a time.
    pub async fn open(&self) -> Result<TxOpen<'_, B::Open>> {
        match self.transaction() {
            Some(tx) if !tx.is_finished() => {}
            Some(_) => return Err(finished()),
            None => {
                // The transaction was dropped unfinished, so no commit can be
                // running: drop what began now, which rolls it back and frees
                // its connection, instead of when the last clone goes.
                if let Ok(mut slot) = self.slot.try_lock() {
                    slot.take();
                }
                return Err(finished());
            }
        }
        let mut slot = self.slot.lock().await;
        // Commit or rollback may have run between the check above and the
        // lock; beginning again would open a transaction nobody finishes.
        if self.closed.load(Ordering::SeqCst) {
            return Err(finished());
        }
        if slot.is_none() {
            let begun = self.backend.begin().await.inspect_err(|_| {
                self.failed.store(true, Ordering::SeqCst);
            })?;
            *slot = Some(begun);
        }
        Ok(TxOpen(slot))
    }

    /// Runs `op`, marking the transaction failed unless it returns `Ok`:
    /// commit then rolls back. Some servers keep a transaction open after a
    /// failed operation, so the server alone wouldn't refuse the commit. An
    /// operation dropped mid-flight (a timeout, a `select!`) counts as failed
    /// too: the server may still have run it.
    pub async fn watch<R, E>(
        &self,
        op: impl IntoFuture<Output = std::result::Result<R, E>>,
    ) -> std::result::Result<R, E> {
        let mut armed = Armed {
            failed: &self.failed,
            settled: false,
        };
        let out = op.await;
        armed.settled = out.is_ok();
        out
    }

    /// Whatever began, taken out for good: later operations are refused.
    async fn close(&self) -> Option<B::Open> {
        let mut slot = self.slot.lock().await;
        self.closed.store(true, Ordering::SeqCst);
        slot.take()
    }
}

/// Marks the transaction failed on drop unless the operation settled `Ok`.
/// It only ever sets the flag, so an earlier failure stays.
struct Armed<'a> {
    failed: &'a AtomicBool,
    settled: bool,
}

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.failed.store(true, Ordering::SeqCst);
        }
    }
}

fn finished() -> VantageError {
    error!("transaction finished")
}

/// The begun transaction's handle, locked. A plain `MutexGuard`, not a mapped
/// one: tokio's `MappedMutexGuard` is `Send` only under a lifetime bound the
/// compiler can't prove inside the async fns that hold it.
pub struct TxOpen<'a, O>(MutexGuard<'a, Option<O>>);

impl<O> Deref for TxOpen<'_, O> {
    type Target = O;

    fn deref(&self) -> &O {
        self.0.as_ref().expect("begun in open()")
    }
}

impl<O> DerefMut for TxOpen<'_, O> {
    fn deref_mut(&mut self) -> &mut O {
        self.0.as_mut().expect("begun in open()")
    }
}

#[async_trait]
impl<B: TxBackend> TxParticipant for TxSlot<B> {
    async fn commit(&self) -> Result<()> {
        let open = self.close().await;
        if self.failed.load(Ordering::SeqCst) {
            // A failed explicit rollback still rolls back: the backend's
            // handle rolls back when it drops, or the server expires it.
            if let Some(open) = open
                && let Err(e) = self.backend.rollback(open).await
            {
                tracing::warn!("{e}");
            }
            return Err(error!(FAILED_STATEMENT));
        }
        let Some(open) = open else {
            return Ok(());
        };
        self.backend.commit(open).await
    }

    async fn rollback(&self) -> Result<()> {
        let Some(open) = self.close().await else {
            return Ok(());
        };
        self.backend.rollback(open).await
    }
}
