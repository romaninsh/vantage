use crate::host::{ScenarioHost, StepFailure, StepRun};
use crate::report::{Outcome, StepReport};
use crate::tree::ResolvedStep;

enum Attempt {
    /// The clock move or `before` failed: the scene couldn't be set.
    Setup(String),
    /// `do` failed.
    Do(StepFailure),
}

pub(super) async fn run<H: ScenarioHost>(
    host: &H,
    state: &mut H::State,
    step: &ResolvedStep,
) -> StepReport {
    let Some(expected) = &step.fails else {
        return match attempt(host, state, step).await {
            Ok(run) => passed(host, state, step, run).await,
            Err(Attempt::Setup(message)) | Err(Attempt::Do(StepFailure { message, .. })) => {
                failed(step, message, None, StepRun::default())
            }
        };
    };
    let mut trial = match host.fork(state).await {
        Ok(trial) => trial,
        Err(e) => return failed(step, format!("fork: {e}"), None, StepRun::default()),
    };
    match attempt(host, &mut trial, step).await {
        Err(Attempt::Do(f)) if !f.harness && f.message.contains(expected.as_str()) => {
            passed(host, state, step, StepRun::default()).await
        }
        Err(Attempt::Do(f)) if f.harness => failed(
            step,
            format!(
                "expected a failure containing `{expected}`, but the step couldn't run: `{}`",
                f.message
            ),
            None,
            StepRun::default(),
        ),
        Err(Attempt::Do(f)) => failed(
            step,
            format!(
                "expected a failure containing `{expected}`, got: `{}`",
                f.message
            ),
            None,
            StepRun::default(),
        ),
        Err(Attempt::Setup(message)) => failed(step, message, None, StepRun::default()),
        Ok(run) => failed(
            step,
            format!("expected a failure containing `{expected}`, but the step passed"),
            None,
            run,
        ),
    }
}

async fn attempt<H: ScenarioHost>(
    host: &H,
    state: &mut H::State,
    step: &ResolvedStep,
) -> Result<StepRun, Attempt> {
    if let Some(by) = &step.clock {
        host.advance(state, by)
            .await
            .map_err(|e| Attempt::Setup(format!("clock {by}: {e}")))?;
    }
    if let Some(rhai) = &step.before {
        host.before(state, rhai)
            .await
            .map_err(|e| Attempt::Setup(format!("before: {e}")))?;
    }
    match &step.do_ {
        Some(_) => host.run(state, step).await.map_err(Attempt::Do),
        None => Ok(StepRun::default()),
    }
}

/// The step ran (or failed as `fails:` expected): declare its vars, then check its asserts.
/// A `fails:` step declares nothing and asserts against the state it leaves, with a null result.
async fn passed<H: ScenarioHost>(
    host: &H,
    state: &mut H::State,
    step: &ResolvedStep,
    run: StepRun,
) -> StepReport {
    if !step.vars.is_empty()
        && let Err(e) = host.declare(state, &step.vars, &run).await
    {
        return failed(step, format!("vars: {e}"), None, run);
    }
    for expr in &step.assert {
        match host.assert(state, expr, &run).await {
            Ok(true) => {}
            Ok(false) => return failed(step, "assert is false".into(), Some(expr), run),
            Err(e) => return failed(step, e.to_string(), Some(expr), run),
        }
    }
    StepReport {
        step: step.step.clone(),
        who: step.session.as_ref().map(|s| s.label()),
        outcome: Outcome::Passed,
        result: run.result,
        writes: run.writes,
    }
}

fn failed(
    step: &ResolvedStep,
    message: String,
    assert: Option<&String>,
    run: StepRun,
) -> StepReport {
    StepReport {
        step: step.step.clone(),
        who: step.session.as_ref().map(|s| s.label()),
        outcome: Outcome::Failed {
            message,
            assert: assert.cloned(),
        },
        result: run.result,
        writes: run.writes,
    }
}
