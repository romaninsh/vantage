//! Writes through a [`Transaction`] land together or not at all.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Barrier;
use tokio::time::{sleep, timeout};
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;

use super::Probe;
use crate::{CborValue, FilterOp, Transaction, Vista};

type Rec = Record<CborValue>;

pub(super) const PROBE_DEADLINE: Duration = Duration::from_secs(2);
const COLLIDE_DEADLINE: Duration = Duration::from_secs(3);
/// The whole check's time, so its one panic lands before nextest's 5 s kill.
const CHECK_BUDGET: Duration = Duration::from_millis(4500);
/// How long `writers_collide` gives the second writer to (wrongly) get through.
const WRITER_WAIT: Duration = Duration::from_millis(100);
/// How long `lock_makes_a_reader_wait` gives the second reader to (wrongly) get through.
const READER_WAIT: Duration = Duration::from_millis(200);
pub(super) const FAILED_STATEMENT: &str = "a statement failed inside the transaction; rolled back";
const LOCK_NEEDS_TX: &str = "lock() needs a transaction";

/// Two empty tables on one datasource:
///   a(id text pk, n int NOT NULL CHECK (n >= 0))
///   b(id text pk, a_id text, n int NOT NULL CHECK (n >= 0))
/// with relation "b" on a (b.a_id = a.id). The check constraint gives
/// `failed_statement_fails_commit` a statement that must fail.
#[non_exhaustive]
pub struct TxTables {
    pub a: Vista,
    pub b: Vista,
}

impl TxTables {
    pub fn new(a: Vista, b: Vista) -> Self {
        Self { a, b }
    }
}

#[async_trait]
pub trait TxFixture: Send + Sync {
    /// New, empty tables on the normal pool.
    async fn fresh(&self) -> TxTables;
    /// New, empty tables, on the same datasource reopened with one connection.
    async fn fresh_pool_of_one(&self) -> TxTables;
    /// Postgres/MySQL report a deadlock; SQLite serialises writers instead.
    fn detects_deadlocks(&self) -> bool;
    /// Whether a wrapped Vista's `lock()` locks the rows it reads.
    fn supports_lock(&self) -> bool {
        false
    }
}

/// Runs every transaction probe on fresh tables, one after another, then
/// panics once, listing the probes that broke the contract, hung past their
/// deadline, or didn't run because the check's time budget was spent. Reads
/// "outside" the transaction go through the unwrapped Vistas.
pub async fn check_transactions(fixture: &impl TxFixture) {
    let mut runner = Runner::new();
    let fx = fixture;
    run(
        &mut runner,
        "commit_lands_every_write",
        PROBE_DEADLINE,
        commit_lands_every_write(fx),
    )
    .await;
    run(
        &mut runner,
        "rollback_lands_none",
        PROBE_DEADLINE,
        rollback_lands_none(fx),
    )
    .await;
    run(
        &mut runner,
        "drop_lands_none",
        PROBE_DEADLINE,
        drop_lands_none(fx),
    )
    .await;
    run(
        &mut runner,
        "uncommitted_is_invisible_outside",
        PROBE_DEADLINE,
        uncommitted_is_invisible_outside(fx),
    )
    .await;
    run(
        &mut runner,
        "reads_see_own_writes",
        PROBE_DEADLINE,
        reads_see_own_writes(fx),
    )
    .await;
    run(
        &mut runner,
        "nothing_begins_until_used",
        PROBE_DEADLINE,
        nothing_begins_until_used(fx),
    )
    .await;
    run(
        &mut runner,
        "ref_target_joins_the_transaction",
        PROBE_DEADLINE,
        ref_target_joins_the_transaction(fx),
    )
    .await;
    run(
        &mut runner,
        "narrowed_vista_keeps_narrowing",
        PROBE_DEADLINE,
        narrowed_vista_keeps_narrowing(fx),
    )
    .await;
    run(
        &mut runner,
        "concurrent_calls_share_the_transaction",
        PROBE_DEADLINE,
        concurrent_calls_share_the_transaction(fx),
    )
    .await;
    run(
        &mut runner,
        "failed_statement_fails_commit",
        PROBE_DEADLINE,
        failed_statement_fails_commit(fx),
    )
    .await;
    run(
        &mut runner,
        "use_after_finish_errors",
        PROBE_DEADLINE,
        use_after_finish_errors(fx),
    )
    .await;
    run(
        &mut runner,
        "writers_collide",
        COLLIDE_DEADLINE,
        writers_collide(fx),
    )
    .await;
    if fx.supports_lock() {
        run(
            &mut runner,
            "lock_outside_a_transaction_errors",
            PROBE_DEADLINE,
            lock_outside_a_transaction_errors(fx),
        )
        .await;
        run(
            &mut runner,
            "lock_makes_a_reader_wait",
            PROBE_DEADLINE,
            lock_makes_a_reader_wait(fx),
        )
        .await;
    }
    runner.finish();
}

/// Runs the probes in turn, each under its own deadline cut to what is left
/// of [`CHECK_BUDGET`].
pub(super) struct Runner {
    probe: Probe,
    ends: Instant,
}

impl Runner {
    pub(super) fn new() -> Self {
        Self {
            probe: Probe::default(),
            ends: Instant::now() + CHECK_BUDGET,
        }
    }

    #[track_caller]
    pub(super) fn finish(self) {
        self.probe.finish();
    }
}

pub(super) async fn run(
    runner: &mut Runner,
    name: &str,
    deadline: Duration,
    body: impl Future<Output = Result<()>>,
) {
    let left = runner.ends.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return runner
            .probe
            .fail(format!("{name}: not run (time budget spent)"));
    }
    match timeout(deadline.min(left), body).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => runner.probe.fail(format!("{name}: {e}")),
        Err(_) => runner.probe.fail(format!("{name}: hung")),
    }
}

async fn commit_lands_every_write(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    tx.wrap(&t.a).insert_value("x", &rec(1)).await?;
    tx.wrap(&t.b).insert_value("y", &rec_b("x", 1)).await?;
    tx.commit().await?;
    present(&t.a, "x", "after commit").await?;
    present(&t.b, "y", "after commit").await
}

async fn rollback_lands_none(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    tx.wrap(&t.a).insert_value("x", &rec(1)).await?;
    tx.rollback().await?;
    absent(&t.a, "x", "after rollback").await
}

async fn drop_lands_none(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh_pool_of_one().await;
    {
        let tx = Transaction::new();
        let a = tx.wrap(&t.a);
        a.insert_value("x", &rec(1)).await?;
    }
    absent(&t.a, "x", "after dropping the transaction").await
}

async fn uncommitted_is_invisible_outside(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    tx.wrap(&t.a).insert_value("x", &rec(1)).await?;
    absent(&t.a, "x", "before commit").await?;
    tx.commit().await?;
    Ok(())
}

async fn reads_see_own_writes(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    let a = tx.wrap(&t.a);
    a.insert_value("x", &rec(1)).await?;
    a.patch_value("x", &rec(2)).await?;
    let n = n_of(&a, "x").await?;
    tx.rollback().await?;
    ensure(
        n == int(2),
        format!("a wrapped get after patching n to 2 reads {n:?}"),
    )
}

async fn nothing_begins_until_used(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh_pool_of_one().await;
    let tx = Transaction::new();
    let _unused = tx.wrap(&t.a);
    t.a.count().await?;
    tx.commit().await?;
    Ok(())
}

async fn ref_target_joins_the_transaction(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh_pool_of_one().await;
    let tx = Transaction::new();
    let a = tx.wrap(&t.a);
    a.insert_value("x", &rec(1)).await?;
    let row_x: Rec = [
        ("id".to_string(), CborValue::Text("x".into())),
        ("n".to_string(), int(1)),
    ]
    .into_iter()
    .collect();
    let b = a.get_ref("b", Some(&row_x))?;
    b.insert_value("y", &rec_b("x", 1)).await?;
    tx.commit().await?;
    present(&t.b, "y", "after commit").await
}

async fn narrowed_vista_keeps_narrowing(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    t.a.insert_value("x", &rec(1)).await?;
    t.a.insert_value("z", &rec(2)).await?;
    let narrowed = t.a.clone().with_condition("n", FilterOp::Eq, int(1))?;
    let tx = Transaction::new();
    let listed = tx.wrap(&narrowed).list_values().await;
    tx.rollback().await?;
    let ids: Vec<String> = listed?.into_keys().collect();
    ensure(
        ids == ["x"],
        format!("a wrapped list narrowed to n = 1 returns {ids:?}, not [\"x\"]"),
    )
}

async fn concurrent_calls_share_the_transaction(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh_pool_of_one().await;
    let tx = Transaction::new();
    let a = tx.wrap(&t.a);
    let (one, two) = (rec(1), rec(2));
    let (x, z) = tokio::join!(a.insert_value("x", &one), a.insert_value("z", &two));
    x?;
    z?;
    tx.commit().await?;
    present(&t.a, "x", "after commit").await?;
    present(&t.a, "z", "after commit").await
}

async fn failed_statement_fails_commit(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    let a = tx.wrap(&t.a);
    a.insert_value("x", &rec(1)).await?;
    ensure(
        a.insert_value("z", &rec(-1)).await.is_err(),
        "an insert of z with n = -1 passed the check constraint".to_string(),
    )?;
    match tx.commit().await {
        Ok(_) => return Err(error!("commit after a failed statement is Ok")),
        Err(e) if !e.to_string().contains(FAILED_STATEMENT) => {
            return Err(error!(format!(
                "commit after a failed statement errs with `{e}`, not `{FAILED_STATEMENT}`"
            )));
        }
        Err(_) => {}
    }
    absent(&t.a, "x", "after the failed commit").await
}

async fn use_after_finish_errors(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    let tx = Transaction::new();
    let a = tx.wrap(&t.a);
    tx.commit().await?;
    match a.list_values().await {
        Ok(_) => Err(error!("a wrapped list after commit is Ok")),
        Err(e) if e.to_string().contains("transaction finished") => Ok(()),
        Err(e) => Err(error!(format!(
            "a wrapped list after commit errs with `{e}`, not `transaction finished`"
        ))),
    }
}

async fn writers_collide(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    t.a.insert_value("x", &rec(1)).await?;
    t.a.insert_value("z", &rec(1)).await?;
    let (tx1, tx2) = (Transaction::new(), Transaction::new());
    let (a1, a2) = (tx1.wrap(&t.a), tx2.wrap(&t.a));

    if fx.detects_deadlocks() {
        let barrier = Barrier::new(2);
        let (r1, r2) = tokio::join!(
            cross(&a1, &tx1, &barrier, "x", "z", 2),
            cross(&a2, &tx2, &barrier, "z", "x", 3),
        );
        return match (r1, r2) {
            (Ok(()), Err(_)) | (Err(_), Ok(())) => Ok(()),
            (Ok(()), Ok(())) => Err(error!(
                "both crossed writers committed; one should have failed on the deadlock"
            )),
            (Err(e1), Err(e2)) => Err(error!(format!(
                "neither crossed writer committed: tx1: {e1}; tx2: {e2}"
            ))),
        };
    }

    a1.patch_value("x", &rec(2)).await?;
    let second_done = AtomicBool::new(false);
    let second = async {
        let patched = a2.patch_value("z", &rec(3)).await;
        second_done.store(true, Ordering::SeqCst);
        patched?;
        tx2.commit().await.map(drop)
    };
    let first = async {
        sleep(WRITER_WAIT).await;
        let waited = !second_done.load(Ordering::SeqCst);
        (waited, tx1.commit().await.map(drop))
    };
    let ((waited, first), second) = tokio::join!(first, second);
    first?;
    second?;
    ensure(
        waited,
        "tx2 patched z while tx1 held an open write".to_string(),
    )?;
    let (x, z) = (n_of(&t.a, "x").await?, n_of(&t.a, "z").await?);
    ensure(
        x == int(2) && z == int(3),
        format!("after both commits x.n = {x:?} and z.n = {z:?}, not 2 and 3"),
    )
}

async fn lock_outside_a_transaction_errors(fx: &impl TxFixture) -> Result<()> {
    let mut a = fx.fresh().await.a;
    match a.lock() {
        Ok(()) => Err(error!("lock() on an unwrapped vista is Ok")),
        Err(e) if e.to_string().contains(LOCK_NEEDS_TX) => Ok(()),
        Err(e) => Err(error!(format!(
            "lock() on an unwrapped vista errs with `{e}`, not `{LOCK_NEEDS_TX}`"
        ))),
    }
}

/// tx1 reads x locked; tx2's locked read of x waits until tx1 commits its
/// patch, then sees it.
async fn lock_makes_a_reader_wait(fx: &impl TxFixture) -> Result<()> {
    let t = fx.fresh().await;
    t.a.insert_value("x", &rec(1)).await?;
    let (tx1, tx2) = (Transaction::new(), Transaction::new());
    let (mut a1, mut a2) = (tx1.wrap(&t.a), tx2.wrap(&t.a));
    a1.lock()?;
    a2.lock()?;
    a1.get_value("x").await?;

    let second_done = AtomicBool::new(false);
    let second = async {
        let n = n_of(&a2, "x").await;
        second_done.store(true, Ordering::SeqCst);
        tx2.rollback().await?;
        n
    };
    let first = async {
        sleep(READER_WAIT).await;
        let waited = !second_done.load(Ordering::SeqCst);
        let committed = async {
            a1.patch_value("x", &rec(2)).await?;
            tx1.commit().await.map(drop)
        }
        .await;
        // Releases x, so the waiting reader returns. Rollback itself is always Ok.
        if committed.is_err() {
            let _ = tx1.rollback().await;
        }
        (waited, committed)
    };
    let ((waited, first), n) = tokio::join!(first, second);
    first?;
    let n = n?;
    ensure(waited, "tx2 read x while tx1 held it locked".to_string())?;
    ensure(
        n == int(2),
        format!("tx2's locked read after tx1 committed n = 2 reads {n:?}"),
    )
}

/// One of two crossed writers: patch `first`, meet the other at `barrier`,
/// then patch `second` and commit. Rolls back on any error. Meets the
/// barrier even when the first patch fails, so the other side never waits
/// for it.
async fn cross(
    a: &Vista,
    tx: &Transaction,
    barrier: &Barrier,
    first: &str,
    second: &str,
    n: i64,
) -> Result<()> {
    let patched = a.patch_value(first, &rec(n)).await;
    barrier.wait().await;
    let out = async {
        patched?;
        a.patch_value(second, &rec(n)).await?;
        tx.commit().await.map(drop)
    }
    .await;
    if out.is_err() {
        tx.rollback().await?;
    }
    out
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn rec(n: i64) -> Rec {
    [("n".to_string(), int(n))].into_iter().collect()
}

fn rec_b(a_id: &str, n: i64) -> Rec {
    [
        ("a_id".to_string(), CborValue::Text(a_id.into())),
        ("n".to_string(), int(n)),
    ]
    .into_iter()
    .collect()
}

pub(super) fn ensure(holds: bool, broken: String) -> Result<()> {
    if holds { Ok(()) } else { Err(error!(broken)) }
}

pub(super) async fn present(vista: &Vista, id: &str, when: &str) -> Result<()> {
    match vista.get_value(id).await? {
        Some(_) => Ok(()),
        None => Err(error!(format!(
            "{}/{id} is not found outside {when}",
            vista.name()
        ))),
    }
}

pub(super) async fn absent(vista: &Vista, id: &str, when: &str) -> Result<()> {
    match vista.get_value(id).await? {
        Some(_) => Err(error!(format!(
            "{}/{id} is found outside {when}",
            vista.name()
        ))),
        None => Ok(()),
    }
}

async fn n_of(vista: &Vista, id: &str) -> Result<CborValue> {
    let row = vista
        .get_value(id)
        .await?
        .ok_or_else(|| error!(format!("{}/{id} is not found", vista.name())))?;
    Ok(row.get("n").cloned().unwrap_or(CborValue::Null))
}
