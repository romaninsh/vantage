use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_rhai::rhai::{Dynamic, EvalAltResult, Scope};
use vantage_rhai::{Host, Limits};
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{
    ATOMIC_ARM_TIMEOUT, ATOMIC_BLOCK_TIMEOUT, AtomicScope, AtomicVocab, DataVocab, ResolverCell,
    Stop, Terminals, Writes,
};
use vantage_vista::{
    TableShell, Transaction, TxKey, TxParticipant, Vista, VistaResolver, forward_table_shell,
};

type Log = Arc<Mutex<Vec<&'static str>>>;

/// One datasource's share of a transaction: records how it finished. With
/// `slow_fail`, its commit takes 300 ms and fails.
struct Recorder {
    log: Log,
    slow_fail: bool,
}

#[async_trait]
impl TxParticipant for Recorder {
    async fn commit(&self) -> Result<()> {
        if self.slow_fail {
            std::thread::sleep(Duration::from_millis(300));
            return Err(error!("commit failed"));
        }
        self.log.lock().unwrap().push("commit");
        Ok(())
    }
    async fn rollback(&self) -> Result<()> {
        self.log.lock().unwrap().push("rollback");
        Ok(())
    }
}

/// A shell on one datasource (`source`) that joins a transaction when rebound.
#[derive(Clone)]
struct Joining {
    inner: Box<dyn TableShell>,
    source: Arc<()>,
    slow_fail: bool,
    log: Log,
}

forward_table_shell!(Joining, inner, {
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        let (log, slow_fail) = (self.log.clone(), self.slow_fail);
        tx.participant(TxKey::of(&self.source), || Recorder { log, slow_fail })?;
        Ok(Box::new(self.clone()))
    }
});

struct Fixture {
    host: Host,
    log: Log,
    committed: Arc<Mutex<Vec<Vec<String>>>>,
}

/// Table `slow_fail` is on a second datasource whose commit is slow and
/// fails; every other name is on one datasource.
fn fixture(limit: Duration) -> Fixture {
    let log = Log::default();
    let committed = Arc::new(Mutex::new(Vec::new()));
    let (source, second) = (Arc::new(()), Arc::new(()));
    let shell_log = log.clone();
    let base: Arc<dyn VistaResolver> = Arc::new(move |name: &str| {
        let log = shell_log.clone();
        let (source, slow_fail) = match name {
            "slow_fail" => (second.clone(), true),
            _ => (source.clone(), false),
        };
        Ok(
            Vista::new(name, MockShell::new()).map_shell(|inner| Joining {
                inner,
                source,
                slow_fail,
                log,
            }),
        )
    });
    let cell = ResolverCell::new(base);
    let seen = committed.clone();
    let scope = AtomicScope::new(cell.clone())
        .with_limit(limit)
        .with_arm_budget(Duration::from_millis(200))
        .on_commit(Arc::new(move |c| {
            seen.lock().unwrap().push(c.written.clone())
        }));
    let clock = scope.clock();
    let terminals = Terminals::ReadWrite {
        limit: None,
        writes: Writes::Allowed,
    };
    let host = Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(Arc::new(cell)))
        .vocab(AtomicVocab::new(scope))
        .vocab_fn(move |engine| {
            engine.on_progress(move |_| {
                clock.should_stop(false).map(|stop| {
                    Dynamic::from(match stop {
                        Stop::BlockTimeout => ATOMIC_BLOCK_TIMEOUT,
                        Stop::ArmTimeout => ATOMIC_ARM_TIMEOUT,
                        _ => "cancelled",
                    })
                })
            });
        })
        .build();
    Fixture {
        host,
        log,
        committed,
    }
}

impl Fixture {
    fn eval(&self, scope: &mut Scope, script: &str) -> std::result::Result<Dynamic, String> {
        self.host
            .engine()
            .eval_with_scope::<Dynamic>(scope, script)
            .map_err(|e| message(&e))
    }

    fn log(&self) -> Vec<&'static str> {
        self.log.lock().unwrap().clone()
    }
}

/// What the script threw, else the error's text.
fn message(err: &EvalAltResult) -> String {
    match err {
        EvalAltResult::ErrorRuntime(value, _) => value.to_string(),
        other => other.to_string(),
    }
}

#[test]
fn atomic_commits_on_success() {
    let f = fixture(Duration::from_secs(2));

    let id = f
        .eval(
            &mut Scope::new(),
            r#"atomic { table("a").insert(#{ name: "pen" }) }"#,
        )
        .unwrap();

    assert!(id.is_string(), "the block's value is its last statement's");
    assert_eq!(f.log(), ["commit"]);
    assert_eq!(*f.committed.lock().unwrap(), [vec!["a".to_string()]]);
}

#[test]
fn atomic_rolls_back_on_throw_and_runs_the_arm() {
    let f = fixture(Duration::from_secs(2));
    let mut scope = Scope::new();

    let err = f
        .eval(
            &mut scope,
            r#"
            fn add() {
                table("a").insert(#{ name: "pen" });
                throw "no";
            }
            let log = ();
            atomic { add() } rollback (e) { log = e; }
            "#,
        )
        .unwrap_err();

    assert!(err.contains("no"), "{err}");
    assert_eq!(f.log(), ["rollback"]);
    // A throw inside a fn reaches the arm as thrown, not wrapped in the call.
    assert_eq!(scope.get_value::<String>("log").as_deref(), Some("no"));
    assert!(!scope.contains("e"), "the arm's binding is rewound");
    assert!(f.committed.lock().unwrap().is_empty());
}

#[test]
fn nested_atomic_is_refused() {
    let f = fixture(Duration::from_secs(2));

    let err = f
        .eval(
            &mut Scope::new(),
            r#"atomic { table("a").insert(#{ name: "pen" }); atomic { 1 } }"#,
        )
        .unwrap_err();

    assert!(err.contains("atomic can't be nested"), "{err}");
    assert_eq!(f.log(), ["rollback"]);
}

#[test]
fn break_out_of_atomic_is_an_error() {
    let f = fixture(Duration::from_secs(2));

    let err = f
        .eval(
            &mut Scope::new(),
            r#"for i in 0..1 { atomic { table("a").insert(#{ name: "pen" }); break; } }"#,
        )
        .unwrap_err();

    assert!(
        err.contains("break and continue can't leave an atomic block"),
        "{err}"
    );
    assert_eq!(f.log(), ["rollback"]);
}

#[test]
fn the_deadline_stops_a_long_block() {
    let f = fixture(Duration::from_millis(200));

    let err = f
        .eval(
            &mut Scope::new(),
            r#"atomic { table("a").insert(#{ name: "pen" }); loop {} }"#,
        )
        .unwrap_err();

    assert!(err.contains("atomic block ran out of time"), "{err}");
    assert_eq!(f.log(), ["rollback"]);
}

#[test]
fn a_commit_failing_past_the_deadline_keeps_its_error() {
    let f = fixture(Duration::from_millis(200));

    let err = f
        .eval(
            &mut Scope::new(),
            r#"atomic { table("slow_fail").insert(#{ name: "pen" }) }"#,
        )
        .unwrap_err();

    assert!(err.contains("commit failed"), "{err}");
}
