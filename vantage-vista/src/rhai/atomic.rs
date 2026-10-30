//! `atomic { BODY } rollback (NAME) { ARM }`: every table a block touches
//! reads and writes through one [`Transaction`], committed when the block
//! ends and rolled back when it throws, is cancelled or runs out of time.
//!
//! [`AtomicScope`] is the Rust core a host also runs an `atomic: true` body
//! through; [`AtomicVocab`] registers the syntax over it. The time limits
//! reach the engine through the host's `on_progress`, which asks
//! [`AtomicClock::should_stop`].

use std::cell::RefCell;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult, EvalContext, Expression};
use vantage_rhai::{Vocab, register_atomic_syntax};

use super::bridge::block_on;
use super::cell::ResolverCell;
use super::read::{RhaiResult, rhai_err};
use crate::transaction::{Committed, Transaction};

pub const ATOMIC_BLOCK_TIMEOUT: &str = "atomic block ran out of time";
pub const ATOMIC_ARM_TIMEOUT: &str = "atomic rollback arm ran out of time";
const NESTED: &str = "atomic can't be nested";
const LEFT_BY_LOOP: &str = "break and continue can't leave an atomic block";

/// Called with what a block committed, e.g. to refresh the written tables.
pub type OnCommit = Arc<dyn Fn(&Committed) + Send + Sync>;

type Arm<'a> = &'a mut dyn FnMut(Box<EvalAltResult>) -> RhaiResult<()>;

/// One per run; never shared across runs.
#[derive(Clone)]
pub struct AtomicScope(Arc<ScopeInner>);

#[derive(Clone)]
struct ScopeInner {
    cell: ResolverCell,
    limit: Duration,
    arm_budget: Duration,
    on_commit: Option<OnCommit>,
    clock: AtomicClock,
}

/// Why the engine must stop now. The host maps each to the value its
/// `on_progress` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stop {
    Cancelled,
    /// [`ATOMIC_BLOCK_TIMEOUT`]
    BlockTimeout,
    /// [`ATOMIC_ARM_TIMEOUT`]
    ArmTimeout,
}

#[derive(Clone, Default)]
pub struct AtomicClock(Arc<ClockInner>);

#[derive(Default)]
struct ClockInner {
    phase: Mutex<Phase>,
}

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Idle,
    Block {
        deadline: Instant,
    },
    Arm {
        deadline: Instant,
    },
}

/// Puts the clock back to idle when dropped.
struct Leave<'a>(&'a AtomicClock);

impl AtomicScope {
    pub fn new(cell: ResolverCell) -> Self {
        Self(Arc::new(ScopeInner {
            cell,
            limit: Duration::from_secs(30),
            arm_budget: Duration::from_secs(10),
            on_commit: None,
            clock: AtomicClock::default(),
        }))
    }

    pub fn with_limit(self, limit: Duration) -> Self {
        self.map(|inner| inner.limit = limit)
    }

    pub fn with_arm_budget(self, budget: Duration) -> Self {
        self.map(|inner| inner.arm_budget = budget)
    }

    pub fn on_commit(self, hook: OnCommit) -> Self {
        self.map(|inner| inner.on_commit = Some(hook))
    }

    fn map(self, change: impl FnOnce(&mut ScopeInner)) -> Self {
        let mut inner = Arc::unwrap_or_clone(self.0);
        change(&mut inner);
        Self(Arc::new(inner))
    }

    pub fn clock(&self) -> AtomicClock {
        self.0.clock.clone()
    }

    /// Inside a block, or an `atomic: true` body. Not in a rollback arm.
    pub fn is_active(&self) -> bool {
        matches!(*self.0.clock.phase(), Phase::Block { .. })
    }

    /// Runs `body` in a new transaction: commit on Ok, rollback on Err, then
    /// `arm(err)` if given. A Rhai `return` out of `body` commits and carries
    /// on outward. Refuses to start inside another block or its arm.
    pub fn run<T>(&self, body: impl FnOnce() -> RhaiResult<T>, arm: Option<Arm>) -> RhaiResult<T> {
        let ScopeInner {
            cell,
            limit,
            arm_budget,
            clock,
            ..
        } = &*self.0;
        let deadline = Instant::now() + *limit;
        let tx = Transaction::new();
        let outcome = {
            let _block = clock.enter(Phase::Block { deadline })?;
            let _pointed = cell.point(tx.resolver(cell.base()));
            body()
        };

        let err = match outcome {
            Ok(value) => match self.commit(&tx) {
                Ok(()) => return Ok(value),
                Err(err) => err,
            },
            // A `return` or `exit` leaving the block isn't a failure.
            Err(ret) if matches!(*ret, EvalAltResult::Return(..) | EvalAltResult::Exit(..)) => {
                match self.commit(&tx) {
                    Ok(()) => return Err(ret),
                    Err(err) => err,
                }
            }
            Err(err) => err,
        };

        if let Err(failed) = block_on(tx.rollback()) {
            tracing::warn!("atomic rollback failed: {failed}");
        }
        // Rhai parses `break` inside the block when it sits in a loop.
        let replaced = if matches!(*err, EvalAltResult::LoopBreak(..)) {
            Some(LEFT_BY_LOOP)
        } else if Instant::now() >= deadline {
            Some(ATOMIC_BLOCK_TIMEOUT)
        } else {
            None
        };
        let err = match replaced {
            Some(text) => Box::new(EvalAltResult::ErrorRuntime(text.into(), err.position())),
            None => err,
        };
        // The block has left, so the clock is idle and entering can't fail.
        if let Some(arm) = arm
            && let Ok(_arm) = clock.enter(Phase::Arm {
                deadline: Instant::now() + *arm_budget,
            })
            && let Err(failed) = arm(for_arm(&err))
        {
            tracing::warn!("atomic rollback arm failed: {failed}");
        }
        Err(err)
    }

    fn commit(&self, tx: &Transaction) -> RhaiResult<()> {
        let committed = block_on(tx.commit()).and_then(|r| r).map_err(rhai_err)?;
        if let Some(hook) = &self.0.on_commit {
            hook(&committed);
        }
        Ok(())
    }
}

impl AtomicClock {
    /// For a host's `on_progress`: `Some` when the engine must stop now. A
    /// rollback arm ignores a cancel and stops only past its own budget.
    pub fn should_stop(&self, cancelled: bool) -> Option<Stop> {
        let now = Instant::now();
        match *self.phase() {
            Phase::Arm { deadline } => (now >= deadline).then_some(Stop::ArmTimeout),
            Phase::Block { deadline } if now >= deadline => Some(Stop::BlockTimeout),
            _ => cancelled.then_some(Stop::Cancelled),
        }
    }

    /// The time the block or rollback arm under way has left; `None`
    /// outside both. A host bounds its blocking calls by it.
    pub fn remaining(&self) -> Option<Duration> {
        match *self.phase() {
            Phase::Block { deadline } | Phase::Arm { deadline } => {
                Some(deadline.saturating_duration_since(Instant::now()))
            }
            Phase::Idle => None,
        }
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.0.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn enter(&self, phase: Phase) -> RhaiResult<Leave<'_>> {
        let mut current = self.phase();
        if !matches!(*current, Phase::Idle) {
            return Err(NESTED.into());
        }
        *current = phase;
        Ok(Leave(self))
    }
}

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        *self.0.phase() = Phase::Idle;
    }
}

/// The error as the arm sees it: what a `throw` threw, a stop's reason, or
/// the error's text, from inside the script `fn` that raised it if any.
fn for_arm(err: &EvalAltResult) -> Box<EvalAltResult> {
    let value = match err.unwrap_inner() {
        EvalAltResult::ErrorRuntime(value, _) | EvalAltResult::ErrorTerminated(value, _) => {
            value.clone()
        }
        other => other.to_string().into(),
    };
    Box::new(EvalAltResult::ErrorRuntime(value, err.position()))
}

/// Registers `atomic { BODY }` and `atomic { BODY } rollback (NAME) { ARM }`.
pub struct AtomicVocab(AtomicScope);

impl AtomicVocab {
    pub fn new(scope: AtomicScope) -> Self {
        Self(scope)
    }
}

impl Vocab for AtomicVocab {
    fn register(&self, engine: &mut Engine) {
        let scope = self.0.clone();
        register_atomic_syntax(engine, move |context, inputs| {
            eval_atomic(&scope, context, inputs)
        });
    }
}

fn eval_atomic(
    scope: &AtomicScope,
    context: &mut EvalContext,
    inputs: &[Expression],
) -> RhaiResult<Dynamic> {
    let context = RefCell::new(context);
    let body = || context.borrow_mut().eval_expression_tree(&inputs[0]);
    let [_, name, arm] = inputs else {
        return scope.run(body, None);
    };
    let name = name.get_string_value().unwrap_or_default().to_string();
    let mut arm = |err: Box<EvalAltResult>| -> RhaiResult<()> {
        let mut context = context.borrow_mut();
        let bound = match *err {
            EvalAltResult::ErrorRuntime(value, _) => value,
            other => other.to_string().into(),
        };
        let depth = context.scope().len();
        context.scope_mut().push_dynamic(name.as_str(), bound);
        let out = context.eval_expression_tree(arm);
        context.scope_mut().rewind(depth);
        out.map(|_| ())
    };
    scope.run(body, Some(&mut arm))
}
