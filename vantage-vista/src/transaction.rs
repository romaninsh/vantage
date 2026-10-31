mod direct;
mod joining;
mod participant;
mod shell;
mod slot;
mod wrap;

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use futures_core::future::BoxFuture;
use indexmap::{IndexMap, IndexSet};
use vantage_core::{Result, VantageError, error};

pub use direct::direct;
pub use participant::{TxKey, TxParticipant};
pub use slot::{TxBackend, TxOpen, TxSlot};

use joining::Joined;

type Deferred = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;

/// The error-context key under which a failed commit names the tables that
/// had already committed, as a JSON array.
const COMMITTED: &str = "committed";

/// What a successful commit reports.
#[derive(Debug)]
#[non_exhaustive]
pub struct Committed {
    /// Tables written through the transaction, in first-written order.
    pub written: Vec<String>,
    /// Tables written directly, outside the transaction, by shells over a
    /// datasource without transactions ([`direct`]). Their writes landed
    /// when made and no rollback undoes them.
    pub unprotected: Vec<String>,
}

#[derive(Default)]
struct State {
    typed: IndexMap<TxKey, Arc<dyn Any + Send + Sync>>,
    ordered: Vec<(TxKey, Arc<dyn TxParticipant>)>,
    written: IndexSet<String>,
    /// The tables written through each participant, so a failed commit can
    /// name the ones that landed.
    tables_of: IndexMap<TxKey, IndexSet<String>>,
    /// Kept past `finish`: a failed block still reports them.
    unprotected: IndexSet<String>,
    deferred: Vec<Deferred>,
}

/// What `finish` takes out of the state.
struct Taken {
    participants: Vec<(TxKey, Arc<dyn TxParticipant>)>,
    deferred: Vec<Deferred>,
    written: IndexSet<String>,
    tables_of: IndexMap<TxKey, IndexSet<String>>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    finished: AtomicBool,
}

/// A unit of work shared by every backend it touches. Commit runs backend by
/// backend, so it's atomic per backend, not across them: if one fails part-way,
/// the backends before it stay committed and the rest roll back.
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
        // Checked under the state lock, where `finish` sets it: a participant
        // added now would never be committed or rolled back.
        if self.is_finished() {
            return Err(error!("transaction finished"));
        }
        let joined = match state.typed.get(&key) {
            Some(existing) => existing
                .clone()
                .downcast::<P>()
                .map_err(|_| error!("transaction participant under this key is another type"))?,
            None => {
                let made = Arc::new(make());
                state.typed.insert(key, made.clone());
                state.ordered.push((key, made.clone()));
                made
            }
        };
        drop(state);
        // Every rebind that reaches the participant is credited, not only the
        // first: each shell's writes are attributed to it.
        joining::note_key(self, key);
        Ok(joined)
    }

    /// Run after a successful commit, in order. Dropped on rollback.
    pub fn defer(&self, after: Deferred) {
        self.state().deferred.push(after);
    }

    /// Record a table written through this transaction (the written-tables report).
    pub fn note_written(&self, table: &str) {
        self.state().written.insert(table.to_string());
    }

    /// Record a write to `table` by a shell whose rebind joined `joined`.
    fn note_written_by(&self, table: &str, joined: &Joined) {
        let mut state = self.state();
        if joined.direct {
            state.unprotected.insert(table.to_string());
            return;
        }
        state.written.insert(table.to_string());
        for key in &joined.keys {
            state
                .tables_of
                .entry(*key)
                .or_default()
                .insert(table.to_string());
        }
    }

    /// The tables written directly so far ([`Committed::unprotected`]),
    /// also after a rollback or a failed commit.
    pub fn unprotected(&self) -> Vec<String> {
        self.state().unprotected.iter().cloned().collect()
    }

    /// The tables whose participants committed before the failure, in commit
    /// order, read from an error [`commit`](Self::commit) returned. Empty
    /// for any other error, and when nothing had committed.
    pub fn committed_before_failure(err: &VantageError) -> Vec<String> {
        err.context
            .get(COMMITTED)
            .and_then(|tables| serde_json::from_str(tables).ok())
            .unwrap_or_default()
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
    /// rolled back and the error returned; the ones already committed stay
    /// committed, and the error names their tables
    /// ([`committed_before_failure`](Self::committed_before_failure)).
    /// Deferred closures run only after a full success, and their failures
    /// are logged, not returned.
    pub async fn commit(&self) -> Result<Committed> {
        let Taken {
            participants,
            deferred,
            written,
            tables_of,
        } = self.finish()?;

        for (done, (_, participant)) in participants.iter().enumerate() {
            if let Err(mut err) = participant.commit().await {
                Self::roll_back_all(&participants[done + 1..]).await;
                let landed: IndexSet<&String> = participants[..done]
                    .iter()
                    .filter_map(|(key, _)| tables_of.get(key))
                    .flatten()
                    .collect();
                if !landed.is_empty() {
                    let landed: Vec<&String> = landed.into_iter().collect();
                    let landed = serde_json::to_string(&landed).unwrap_or_default();
                    err.context.insert(COMMITTED.to_string(), landed);
                }
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
            unprotected: self.unprotected(),
        })
    }

    pub async fn rollback(&self) -> Result<()> {
        // Rolling back a finished transaction is a no-op, so cleanup paths can call it freely.
        if let Ok(taken) = self.finish() {
            Self::roll_back_all(&taken.participants).await;
        }
        Ok(())
    }

    fn finish(&self) -> Result<Taken> {
        let mut state = self.state();
        if self.0.finished.swap(true, Ordering::SeqCst) {
            return Err(error!("transaction finished"));
        }
        state.typed.clear();
        Ok(Taken {
            participants: std::mem::take(&mut state.ordered),
            deferred: std::mem::take(&mut state.deferred),
            written: std::mem::take(&mut state.written),
            tables_of: std::mem::take(&mut state.tables_of),
        })
    }

    async fn roll_back_all(participants: &[(TxKey, Arc<dyn TxParticipant>)]) {
        for (_, participant) in participants {
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
