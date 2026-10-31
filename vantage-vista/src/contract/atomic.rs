//! A Rhai `atomic {}` block lands every write it makes, or none.

use std::sync::Arc;
use std::time::Duration;

use vantage_core::{Result, error};
use vantage_rhai::rhai::{Dynamic, EvalAltResult, Map, Scope};
use vantage_rhai::{Host, Limits};

use super::transactions::{
    FAILED_STATEMENT, PROBE_DEADLINE, Runner, TxFixture, TxTables, absent, ensure, present, run,
};
use crate::rhai::{
    ATOMIC_ARM_TIMEOUT, ATOMIC_BLOCK_TIMEOUT, AtomicScope, AtomicVocab, DataVocab, ResolverCell,
    Stop, Terminals, Writes,
};
use crate::{Vista, VistaResolver};

const NESTED: &str = "atomic can't be nested";
/// Ends a hung block inside its probe's deadline, so it fails the probe
/// rather than the whole test.
const BLOCK_LIMIT: Duration = Duration::from_secs(1);
const ARM_BUDGET: Duration = Duration::from_millis(200);

/// Runs every `atomic` probe on fresh tables, one after another, under the
/// same budget and deadlines as [`check_transactions`](super::check_transactions),
/// then panics once, listing the probes that broke the contract. Scripts see
/// the tables as `a` and `b` and run on a blocking thread, the way a host
/// runs them; reads "outside" go through the unwrapped Vistas.
pub async fn check_atomic_rhai(fx: &impl TxFixture) {
    let mut runner = Runner::new();
    run(
        &mut runner,
        "atomic_block_commits",
        PROBE_DEADLINE,
        atomic_block_commits(fx),
    )
    .await;
    run(
        &mut runner,
        "a_throw_rolls_back_and_runs_the_arm",
        PROBE_DEADLINE,
        a_throw_rolls_back_and_runs_the_arm(fx),
    )
    .await;
    run(
        &mut runner,
        "a_throwing_arm_rethrows_the_first_error",
        PROBE_DEADLINE,
        a_throwing_arm_rethrows_the_first_error(fx),
    )
    .await;
    run(
        &mut runner,
        "return_inside_atomic_commits",
        PROBE_DEADLINE,
        return_inside_atomic_commits(fx),
    )
    .await;
    run(
        &mut runner,
        "a_handle_from_before_the_block_joins_it",
        PROBE_DEADLINE,
        a_handle_from_before_the_block_joins_it(fx),
    )
    .await;
    run(
        &mut runner,
        "a_handle_outlives_its_block",
        PROBE_DEADLINE,
        a_handle_outlives_its_block(fx),
    )
    .await;
    run(
        &mut runner,
        "a_failed_statement_fails_the_block",
        PROBE_DEADLINE,
        a_failed_statement_fails_the_block(fx),
    )
    .await;
    run(
        &mut runner,
        "nested_atomic_is_refused",
        PROBE_DEADLINE,
        nested_atomic_is_refused(fx),
    )
    .await;
    runner.finish();
}

async fn atomic_block_commits(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"atomic {
            table("a").insert(#{ id: "x", n: 1 });
            table("b").insert(#{ id: "y", a_id: "x", n: 1 });
        }"#,
    )
    .await?;
    if let Err(e) = ran.out {
        return Err(error!(format!("the block throws `{e}`")));
    }
    present(&t.a, "x", "after the block").await?;
    present(&t.b, "y", "after the block").await
}

async fn a_throw_rolls_back_and_runs_the_arm(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"let seen = ();
        atomic {
            table("a").insert(#{ id: "x", n: 1 });
            throw "stop";
        } rollback (e) {
            seen = e;
        }"#,
    )
    .await?;
    threw(&ran, "stop")?;
    ensure(
        ran.seen.as_deref() == Some("stop"),
        format!("the arm binds {:?}, not \"stop\"", ran.seen),
    )?;
    absent(&t.a, "x", "after the throw").await
}

async fn a_throwing_arm_rethrows_the_first_error(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"atomic {
            table("a").insert(#{ id: "x", n: 1 });
            throw "stop";
        } rollback (e) {
            throw "arm";
        }"#,
    )
    .await?;
    threw(&ran, "stop")?;
    absent(&t.a, "x", "after the arm threw").await
}

async fn return_inside_atomic_commits(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"fn f() {
            atomic {
                table("a").insert(#{ id: "x", n: 1 });
                return 7;
            }
        }
        f()"#,
    )
    .await?;
    match &ran.out {
        Ok(v) if v.as_int() == Ok(7) => {}
        Ok(v) => return Err(error!(format!("f() returns {v}, not 7"))),
        Err(e) => return Err(error!(format!("f() throws `{e}`, not returns 7"))),
    }
    present(&t.a, "x", "after the return").await
}

async fn a_handle_from_before_the_block_joins_it(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"let t = table("a");
        atomic {
            t.insert(#{ id: "x", n: 1 });
            throw "stop";
        }"#,
    )
    .await?;
    threw(&ran, "stop")?;
    absent(&t.a, "x", "after the throw").await
}

/// The handle is made inside the block; read after it, it sees the commit.
async fn a_handle_outlives_its_block(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"let h;
        atomic {
            h = table("a");
            h.insert(#{ id: "x", n: 1 });
        }
        h.get("x")"#,
    )
    .await?;
    let row = ran
        .out
        .map_err(|e| error!(format!("the read after the block throws `{e}`")))?;
    let n = row
        .clone()
        .try_cast::<Map>()
        .and_then(|row| row.get("n").and_then(|n| n.as_int().ok()));
    ensure(
        n == Some(1),
        format!("h.get(\"x\") after the block returns {row}, not a row with n = 1"),
    )?;
    present(&t.a, "x", "after the block").await
}

/// The body catches the failed insert and ends Ok, so only the commit can
/// fail the block.
async fn a_failed_statement_fails_the_block(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"atomic {
            table("a").insert(#{ id: "x", n: 1 });
            try { table("a").insert(#{ id: "z", n: -1 }) } catch {}
        }"#,
    )
    .await?;
    let failed = matches!(&ran.out, Err(e) if e.contains(FAILED_STATEMENT));
    ensure(
        failed,
        format!("the block ends {:?}, not `{FAILED_STATEMENT}`", ran.out),
    )?;
    absent(&t.a, "x", "after the failed block").await
}

async fn nested_atomic_is_refused(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let ran = eval(
        &t,
        r#"atomic {
            table("a").insert(#{ id: "x", n: 1 });
            atomic { 1 }
        }"#,
    )
    .await?;
    threw(&ran, NESTED)?;
    absent(&t.a, "x", "after the nested block").await
}

/// What a script returned or threw, and its `seen` variable as it left it.
struct Ran {
    out: std::result::Result<Dynamic, String>,
    seen: Option<String>,
}

/// Runs `script` over the tables on a blocking thread, since table calls
/// block on the runtime. Each run gets its own cell and atomic scope.
async fn eval(t: &TxTables, script: &'static str) -> Result<Ran> {
    let (a, b) = (t.a.clone(), t.b.clone());
    tokio::task::spawn_blocking(move || {
        let host = host(a, b);
        let mut scope = Scope::new();
        let out = host
            .engine()
            .eval_with_scope::<Dynamic>(&mut scope, script)
            .map_err(|e| message(&e));
        Ran {
            out,
            seen: scope.get_value::<String>("seen"),
        }
    })
    .await
    .map_err(|e| error!(format!("the script's thread failed: {e}")))
}

fn host(a: Vista, b: Vista) -> Host {
    let base: Arc<dyn VistaResolver> = Arc::new(move |name: &str| match name {
        "a" => Ok(a.clone()),
        "b" => Ok(b.clone()),
        _ => Err(error!("no such table", name = name)),
    });
    let cell = ResolverCell::new(base);
    let terminals = Terminals::ReadWrite {
        limit: None,
        writes: Writes::Allowed,
    };
    let scope = AtomicScope::new(cell.clone())
        .with_limit(BLOCK_LIMIT)
        .with_arm_budget(ARM_BUDGET);
    let clock = scope.clock();
    Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(Arc::new(cell)))
        .vocab(AtomicVocab::new(scope))
        .vocab_fn(move |engine| {
            engine.on_progress(move |_| {
                clock.should_stop(false).map(|stop| {
                    Dynamic::from(match stop {
                        Stop::Cancelled => "cancelled",
                        Stop::BlockTimeout => ATOMIC_BLOCK_TIMEOUT,
                        Stop::ArmTimeout => ATOMIC_ARM_TIMEOUT,
                    })
                })
            });
        })
        .build()
}

/// What the script threw, else the error's text.
fn message(err: &EvalAltResult) -> String {
    match err {
        EvalAltResult::ErrorRuntime(value, _) => value.to_string(),
        other => other.to_string(),
    }
}

fn threw(ran: &Ran, wanted: &str) -> Result<()> {
    match &ran.out {
        Err(e) if e == wanted => Ok(()),
        Err(e) => Err(error!(format!("the script throws `{e}`, not `{wanted}`"))),
        Ok(v) => Err(error!(format!("the script returns {v}, not `{wanted}`"))),
    }
}
