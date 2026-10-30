mod participant;
mod shell;
mod wrap;

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use futures_core::future::BoxFuture;
use indexmap::{IndexMap, IndexSet};
use vantage_core::{Result, VantageError, error};

pub use participant::{TxKey, TxParticipant};

type Deferred = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;
type Taken = (Vec<Arc<dyn TxParticipant>>, Vec<Deferred>, IndexSet<String>);

/// What a successful commit reports.
#[derive(Debug)]
#[non_exhaustive]
pub struct Committed {
    /// Tables written through the transaction, in first-written order.
    pub written: Vec<String>,
}

#[derive(Default)]
struct State {
    typed: IndexMap<TxKey, Arc<dyn Any + Send + Sync>>,
    ordered: Vec<Arc<dyn TxParticipant>>,
    written: IndexSet<String>,
    deferred: Vec<Deferred>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    finished: AtomicBool,
}

/// A unit of work shared by every backend it touches: they commit or roll
/// back together.
#[derive(Clone)]
pub struct Transaction(Arc<Inner>);

/// A [`Transaction`] handle that doesn't keep it alive.
#[derive(Clone)]
pub struct WeakTransaction(Weak<Inner>);

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

impl Transaction {
    pub fn new() -> Transaction {
        Transaction(Arc::new(Inner::default()))
    }

    pub fn downgrade(&self) -> WeakTransaction {
        WeakTransaction(Arc::downgrade(&self.0))
    }

    pub fn same(&self, other: &Transaction) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The participant for `key`, made by `make` on first use; later calls return the same Arc.
    /// Err if the one stored under `key` is another type.
    pub fn participant<P: TxParticipant>(
        &self,
        key: TxKey,
        make: impl FnOnce() -> P,
    ) -> Result<Arc<P>> {
        let mut state = self.state();
        if let Some(existing) = state.typed.get(&key) {
            return existing
                .clone()
                .downcast::<P>()
                .map_err(|_| error!("transaction participant under this key is another type"));
        }
        let made = Arc::new(make());
        state.typed.insert(key, made.clone());
        state.ordered.push(made.clone());
        Ok(made)
    }

    /// Run after a successful commit, in order. Dropped on rollback.
    pub fn defer(&self, after: Deferred) {
        self.state().deferred.push(after);
    }

    /// Record a table written through this transaction (the written-tables report).
    pub fn note_written(&self, table: &str) {
        self.state().written.insert(table.to_string());
    }

    pub fn is_finished(&self) -> bool {
        self.0.finished.load(Ordering::SeqCst)
    }

    pub fn check_open(&self) -> Result<()> {
        if self.is_finished() {
            return Err(error!("transaction finished"));
        }
        Ok(())
    }

    /// Commits participants in join order. On the first failure the rest are
    /// rolled back and the error returned; deferred closures run only after a
    /// full success, and their failures are logged, not returned.
    pub async fn commit(&self) -> Result<Committed> {
        let (participants, deferred, written) = self.finish()?;

        for (done, participant) in participants.iter().enumerate() {
            if let Err(err) = participant.commit().await {
                Self::roll_back_all(&participants[done + 1..]).await;
                return Err(err);
            }
        }

        for after in deferred {
            if let Err(err) = after().await {
                tracing::warn!("deferred work after commit failed: {err}");
            }
        }
        Ok(Committed {
            written: written.into_iter().collect(),
        })
    }

    pub async fn rollback(&self) -> Result<()> {
        // Rolling back a finished transaction is a no-op, so cleanup paths can call it freely.
        if let Ok((participants, _, _)) = self.finish() {
            Self::roll_back_all(&participants).await;
        }
        Ok(())
    }

    fn finish(&self) -> Result<Taken> {
        if self.0.finished.swap(true, Ordering::SeqCst) {
            return Err(error!("transaction finished"));
        }
        let mut state = self.state();
        state.typed.clear();
        Ok((
            std::mem::take(&mut state.ordered),
            std::mem::take(&mut state.deferred),
            std::mem::take(&mut state.written),
        ))
    }

    async fn roll_back_all(participants: &[Arc<dyn TxParticipant>]) {
        for participant in participants {
            if let Err(err) = participant.rollback().await {
                tracing::warn!("transaction rollback failed: {err}");
            }
        }
    }
}

impl WeakTransaction {
    pub fn upgrade(&self) -> Option<Transaction> {
        self.0.upgrade().map(Transaction)
    }
}

/// The refusal of a table whose shell can't rebind to a transaction. Marked
/// `Unsupported`: [`Transaction::wrap`] renames any `Unsupported` rebind
/// refusal to the wrapped Vista's name.
pub fn cant_take_part(table: &str) -> VantageError {
    error!(format!("\"{table}\" can't take part in a transaction")).mark_unsupported()
}

/// The default `rebind` refusal; a shell doesn't know the name its Vista goes by.
pub(crate) fn refuse_rebind() -> VantageError {
    error!("this shell can't take part in a transaction").mark_unsupported()
}

pub(crate) fn refuse_lock() -> VantageError {
    error!("this shell can't lock rows").mark_unsupported()
}
