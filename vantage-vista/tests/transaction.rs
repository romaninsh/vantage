use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_vista::{Transaction, TxKey, TxParticipant};

type Log = Arc<Mutex<Vec<&'static str>>>;

struct Probe {
    name: &'static str,
    fail_commit: bool,
    log: Log,
}

impl Probe {
    fn record(&self, what: &str) {
        let entry: &'static str = Box::leak(format!("{}:{what}", self.name).into_boxed_str());
        self.log.lock().unwrap().push(entry);
    }
}

#[async_trait]
impl TxParticipant for Probe {
    async fn commit(&self) -> Result<()> {
        self.record("commit");
        if self.fail_commit {
            return Err(error!("commit failed"));
        }
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        self.record("rollback");
        Ok(())
    }
}

struct Other;

#[async_trait]
impl TxParticipant for Other {
    async fn commit(&self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        Ok(())
    }
}

fn join(tx: &Transaction, token: &Arc<()>, name: &'static str, fail: bool, log: &Log) {
    tx.participant(TxKey::of(token), || Probe {
        name,
        fail_commit: fail,
        log: log.clone(),
    })
    .unwrap();
}

#[tokio::test]
async fn participants_commit_in_join_order() {
    let log = Log::default();
    let (a, b) = (Arc::new(()), Arc::new(()));
    let tx = Transaction::new();
    join(&tx, &a, "a", false, &log);
    join(&tx, &b, "b", false, &log);
    tx.commit().await.unwrap();
    assert_eq!(*log.lock().unwrap(), ["a:commit", "b:commit"]);
}

#[tokio::test]
async fn a_failed_commit_rolls_back_the_rest() {
    let log = Log::default();
    let (a, b) = (Arc::new(()), Arc::new(()));
    let tx = Transaction::new();
    join(&tx, &a, "a", true, &log);
    join(&tx, &b, "b", false, &log);
    assert!(tx.commit().await.is_err());
    assert_eq!(*log.lock().unwrap(), ["a:commit", "b:rollback"]);
}

#[tokio::test]
async fn deferred_runs_after_commit_only() {
    let count = Arc::new(AtomicUsize::new(0));
    let bump = |count: &Arc<AtomicUsize>| {
        let count = count.clone();
        Box::new(move || {
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }) as futures_core::future::BoxFuture<'static, Result<()>>
        }) as Box<dyn FnOnce() -> _ + Send>
    };

    let committed = Transaction::new();
    committed.defer(bump(&count));
    committed.commit().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);

    let rolled_back = Transaction::new();
    rolled_back.defer(bump(&count));
    rolled_back.rollback().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn same_key_same_participant() {
    let log = Log::default();
    let token = Arc::new(());
    let key = TxKey::of(&token);
    let tx = Transaction::new();
    let first = tx
        .participant(key, || Probe {
            name: "a",
            fail_commit: false,
            log: log.clone(),
        })
        .unwrap();
    let second = tx
        .participant(key, || Probe {
            name: "x",
            fail_commit: false,
            log: log.clone(),
        })
        .unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(tx.participant(key, || Other).is_err());
}

#[tokio::test]
async fn finished_after_commit() {
    let tx = Transaction::new();
    assert!(tx.check_open().is_ok());
    tx.commit().await.unwrap();
    assert!(tx.is_finished());
    let err = tx.check_open().unwrap_err();
    assert!(err.to_string().contains("transaction finished"));
}

#[tokio::test]
async fn written_tables_reported_once_each() {
    let tx = Transaction::new();
    tx.note_written("a");
    tx.note_written("b");
    tx.note_written("a");
    let committed = tx.commit().await.unwrap();
    assert_eq!(committed.written, ["a", "b"]);
}
