use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError};

use crate::file::{ClockOffset, Scenario, ScenarioFile, VarDef};
use crate::report::Write;
use crate::tree::ResolvedStep;

/// Everything that touches data. The engine decides what runs and in which state; the host
/// owns the state and evaluates Rhai.
///
/// A state is a value: [`fork`](ScenarioHost::fork) must return a copy whose writes, ids,
/// clock and random draws never reach the original, and the reverse.
#[async_trait]
pub trait ScenarioHost: Send + Sync {
    type State: Send + Sync;

    /// Called once per file: load its fixtures and check its file-level `vars` against them. An
    /// error here is the file's load error, and nothing runs.
    async fn load(&self, file: &ScenarioFile) -> Result<Self::State>;

    /// The start of a root scenario: a fork of `base` (what [`load`](ScenarioHost::load)
    /// returned) with the scenario's `clock` set and the random source seeded with `seed`
    /// (see [`seed()`](crate::seed())). `scenario` has `extends` applied.
    async fn root(&self, base: &Self::State, scenario: &Scenario, seed: u64)
    -> Result<Self::State>;

    async fn fork(&self, s: &Self::State) -> Result<Self::State>;

    async fn advance(&self, s: &mut Self::State, by: &ClockOffset) -> Result<()>;

    /// A step's `before:`, run as the test: no session, hooks or access rules.
    async fn before(&self, s: &mut Self::State, rhai: &str) -> Result<()>;

    /// The step's `do`, under `step.session` with `step.stub`. Only called when the step has
    /// a `do`. Writes made before an error may stay in `s`: the engine runs a step that may
    /// fail on a fork and drops it.
    async fn run(
        &self,
        s: &mut Self::State,
        step: &ResolvedStep,
    ) -> std::result::Result<StepRun, StepFailure>;

    /// One `assert` expression; `Ok(false)` fails the step, `Err` fails it with the message.
    async fn assert(&self, s: &Self::State, rhai: &str, result: &StepRun) -> Result<bool>;

    /// A step's `vars:`, declared once it has run.
    async fn declare(
        &self,
        s: &mut Self::State,
        vars: &IndexMap<String, VarDef>,
        result: &StepRun,
    ) -> Result<()>;
}

/// What a step's `do` returned and what it wrote.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepRun {
    pub result: serde_json::Value,
    pub writes: Vec<Write>,
}

/// A `do` that failed. A `fails:` step matches its text against `message`, unless `harness` is
/// set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    pub message: String,
    /// The test itself is wrong, not the behaviour under test: an unknown action, a bad `$var`,
    /// a missing stub. Always red; never satisfies `fails:`.
    pub harness: bool,
}

impl StepFailure {
    /// The behaviour failed: a throw, a refused write, a hook's refusal.
    pub fn new(message: impl Into<String>) -> Self {
        StepFailure {
            message: message.into(),
            harness: false,
        }
    }

    pub fn harness(message: impl Into<String>) -> Self {
        StepFailure {
            message: message.into(),
            harness: true,
        }
    }
}

/// A `VantageError` reaching `?` is taken as a harness error; build a throw with
/// [`StepFailure::new`].
impl From<VantageError> for StepFailure {
    fn from(e: VantageError) -> Self {
        StepFailure::harness(e.to_string())
    }
}
