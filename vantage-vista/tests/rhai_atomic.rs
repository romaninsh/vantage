use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_rhai::rhai::{Array, Dynamic, Engine, EvalAltResult, Scope};
use vantage_rhai::{Host, Limits, register_atomic_syntax};
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{
    ATOMIC_ARM_TIMEOUT, ATOMIC_BLOCK_TIMEOUT, AtomicScope, AtomicVocab, DataVocab, ResolverCell,
    Stop, Terminals, Writes, block_on,
};
use vantage_vista::{
    TableShell, Transaction, TxKey, TxParticipant, Vista, VistaResolver, direct,
    forward_table_shell,
};

type Log = Arc<Mutex<Vec<&'static str>>>;

/// Scripts run inside it, so a commit can await a timer; its worker thread
/// drives the timer while the script blocks.
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
});

/// How a table's datasource behaves in a transaction; slow means 300 ms.
#[derive(Clone, Copy, PartialEq)]
enum Quirk {
    None,
    FailingCommit,
    SlowFailingCommit,
    /// Joining the transaction is slow, so the write ends late.
    SlowJoin,
}

/// One datasource's share of a transaction: records how it finished.
struct Recorder {
    log: Log,
    quirk: Quirk,
}

#[async_trait]
impl TxParticipant for Recorder {
    async fn commit(&self) -> Result<()> {
        if self.quirk == Quirk::SlowFailingCommit {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if matches!(self.quirk, Quirk::FailingCommit | Quirk::SlowFailingCommit) {
            self.log.lock().unwrap().push("failed commit");
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
    quirk: Quirk,
    log: Log,
}

forward_table_shell!(Joining, inner, {
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        if self.quirk == Quirk::SlowJoin {
            std::thread::sleep(Duration::from_millis(300));
        }
        let (log, quirk) = (self.log.clone(), self.quirk);
        tx.participant(TxKey::of(&self.source), || Recorder { log, quirk })?;
        Ok(Box::new(self.clone()))
    }
});

/// A shell on a datasource without transactions.
#[derive(Clone)]
struct Untransacted {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Untransacted, inner, {
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(direct(Box::new(self.clone())))
    }
});

struct Fixture {
    host: Host,
    log: Log,
    committed: Arc<Mutex<Vec<Vec<String>>>>,
    unprotected: Arc<Mutex<Vec<Vec<String>>>>,
    /// What `on_committed_before_failure` was told.
    landed: Arc<Mutex<Vec<Vec<String>>>>,
    /// The store behind table `direct`.
    direct: Vista,
}

/// Tables `fails_commit` and `slow_fail` (after 300 ms) are on a second
/// datasource whose commit fails, table `direct` on a datasource without
/// transactions; every other name, `slow_join` joining slowly, is on one
/// transactional datasource.
fn fixture(limit: Duration) -> Fixture {
    let log = Log::default();
    let committed = Arc::new(Mutex::new(Vec::new()));
    let unprotected = Arc::new(Mutex::new(Vec::new()));
    let landed = Arc::new(Mutex::new(Vec::new()));
    let (source, second) = (Arc::new(()), Arc::new(()));
    let direct = Vista::new("direct", MockShell::new()).map_shell(|inner| Untransacted { inner });
    let shell_log = log.clone();
    let direct_table = direct.clone();
    let base: Arc<dyn VistaResolver> = Arc::new(move |name: &str| {
        let log = shell_log.clone();
        let (source, quirk) = match name {
            "direct" => return Ok(direct_table.clone()),
            "fails_commit" => (second.clone(), Quirk::FailingCommit),
            "slow_fail" => (second.clone(), Quirk::SlowFailingCommit),
            "slow_join" => (source.clone(), Quirk::SlowJoin),
            _ => (source.clone(), Quirk::None),
        };
        Ok(
            Vista::new(name, MockShell::new()).map_shell(|inner| Joining {
                inner,
                source,
                quirk,
                log,
            }),
        )
    });
    let cell = ResolverCell::new(base);
    let seen = committed.clone();
    let reported = unprotected.clone();
    let partial = landed.clone();
    let scope = AtomicScope::new(cell.clone())
        .with_limit(limit)
        .with_arm_budget(Duration::from_millis(200))
        .on_commit(Arc::new(move |c| {
            seen.lock().unwrap().push(c.written.clone())
        }))
        .on_unprotected(Arc::new(move |tables| {
            reported.lock().unwrap().push(tables.to_vec())
        }))
        .on_committed_before_failure(Arc::new(move |tables| {
            partial.lock().unwrap().push(tables.to_vec())
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
        unprotected,
        landed,
        direct,
    }
}

impl Fixture {
    fn eval(&self, scope: &mut Scope, script: &str) -> std::result::Result<Dynamic, String> {
        let _runtime = RUNTIME.enter();
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

#[test]
fn a_body_ending_past_the_deadline_rolls_back() {
    let f = fixture(Duration::from_millis(200));

    let err = f
        .eval(
            &mut Scope::new(),
            r#"atomic { table("slow_join").insert(#{ name: "pen" }) }"#,
        )
        .unwrap_err();

    assert!(err.contains("atomic block ran out of time"), "{err}");
    assert_eq!(f.log(), ["rollback"]);
}

/// The second datasource fails its commit after the first committed: the
/// first stays committed, and both the arm's `committed` and the host's
/// hook name every table written on it, in write order.
#[test]
fn committed_names_what_landed() {
    let f = fixture(Duration::from_secs(2));
    let mut scope = Scope::new();

    let err = f
        .eval(
            &mut scope,
            r#"
            let seen = ();
            let landed = ();
            atomic {
                table("b").insert(#{ name: "pen" });
                table("a").insert(#{ name: "pen" });
                table("fails_commit").insert(#{ name: "ink" });
            } rollback (e, committed) {
                seen = e;
                landed = committed;
            }
            "#,
        )
        .unwrap_err();

    assert!(err.contains("commit failed"), "{err}");
    assert_eq!(f.log(), ["commit", "failed commit"]);
    assert_eq!(names(&scope, "landed"), ["b", "a"]);
    assert_eq!(
        *f.landed.lock().unwrap(),
        [vec!["b".to_string(), "a".to_string()]]
    );
    assert!(f.committed.lock().unwrap().is_empty());
    let seen = scope.get_value::<String>("seen").unwrap_or_default();
    assert!(seen.contains("commit failed"), "the arm sees {seen:?}");
    assert!(
        !scope.contains("committed"),
        "the arm's bindings are rewound"
    );
}

/// A read of `b` joins the first datasource; `a`, written after it, still
/// counts as landed and `b`, only read, doesn't.
#[test]
fn committed_lists_a_table_written_after_a_read_joined_its_database() {
    let f = fixture(Duration::from_secs(2));
    let mut scope = Scope::new();

    f.eval(
        &mut scope,
        r#"
        let landed = ();
        atomic {
            table("b").count();
            table("a").insert(#{ name: "pen" });
            table("fails_commit").insert(#{ name: "ink" });
        } rollback (e, committed) {
            landed = committed;
        }
        "#,
    )
    .unwrap_err();

    assert_eq!(f.log(), ["commit", "failed commit"]);
    assert_eq!(names(&scope, "landed"), ["a"]);
}

/// A block over a transactional table and a direct one throws: the
/// transactional write rolls back, the direct one stays and is reported.
#[test]
fn a_direct_write_stays_and_is_reported() {
    let f = fixture(Duration::from_secs(2));
    let mut scope = Scope::new();

    let err = f
        .eval(
            &mut scope,
            r#"
            let seen = ();
            let landed = ();
            atomic {
                table("a").insert(#{ name: "pen" });
                table("direct").insert(#{ name: "ink" });
                throw "no";
            } rollback (e, committed) {
                seen = e;
                landed = committed;
            }
            "#,
        )
        .unwrap_err();

    assert_eq!(err, "no");
    assert_eq!(f.log(), ["rollback"]);
    assert_eq!(scope.get_value::<String>("seen").as_deref(), Some("no"));
    assert!(names(&scope, "landed").is_empty(), "nothing committed");
    assert_eq!(block_on(f.direct.count()).unwrap().unwrap(), 1);
    assert_eq!(*f.unprotected.lock().unwrap(), [vec!["direct".to_string()]]);
    assert!(f.committed.lock().unwrap().is_empty());
    assert!(f.landed.lock().unwrap().is_empty());
}

/// `rollback (e)` hands the evaluator the block, one name and the arm;
/// `rollback (e, committed)` one name more.
#[test]
fn rollback_with_one_name_still_parses() {
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let record = seen.clone();
    let mut engine = Engine::new_raw();
    register_atomic_syntax(&mut engine, move |_, inputs| {
        let names = inputs
            .iter()
            .map(|input| input.get_string_value().unwrap_or("{..}").to_string())
            .collect();
        record.lock().unwrap().push(names);
        Ok(Dynamic::UNIT)
    });

    engine.run("atomic { 1 } rollback (e) { 2 }").unwrap();
    engine
        .run("atomic { 1 } rollback (e, committed) { 2 }")
        .unwrap();
    assert!(
        engine
            .compile("atomic { 1 } rollback (e, c, d) { 2 }")
            .is_err(),
        "a third name parses"
    );

    assert_eq!(
        *seen.lock().unwrap(),
        [
            vec!["{..}", "e", "{..}"],
            vec!["{..}", "e", "committed", "{..}"]
        ]
    );
}

/// The array of strings `name` holds in `scope`.
fn names(scope: &Scope, name: &str) -> Vec<String> {
    let array = scope.get_value::<Array>(name).unwrap_or_default();
    array.into_iter().map(|t| t.to_string()).collect()
}
