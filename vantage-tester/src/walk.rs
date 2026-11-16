mod step;

use crate::file::ScenarioFile;
use crate::host::ScenarioHost;
use crate::report::{FileReport, Outcome, ScenarioReport, Status, StepReport};
use crate::seed::seed;
use crate::tree::{Branch, Node, ResolvedStep, Tree, label};

/// Progress, in walk order. Scenario names are labels (`revoked ← funded`). `FileStarted`
/// carries the number of steps in the tree, for a `12/40` counter.
#[derive(Debug, Clone)]
pub enum Event {
    FileStarted { path: String, total: usize },
    ScenarioStarted { scenario: String },
    ScenarioSkipped { scenario: String, reason: String },
    StepStarted { scenario: String, step: String },
    StepFinished { scenario: String, step: StepReport },
}

/// How a queued scenario starts.
enum Start<S> {
    Ready(S),
    /// Reported with every step `NotRun` for this reason; not an error of its own.
    NotRun(&'static str),
    /// `root` or `fork` failed: the scenario is red with this error.
    Failed(String),
}

struct Job<'t, S> {
    node: &'t Node,
    start: Start<S>,
    chain: Vec<String>,
}

const EARLIER_STEP: &str = "an earlier step failed";
const PARENT_STEP: &str = "parent step failed";
const NOT_STARTED: &str = "the scenario didn't start";

/// Runs every root scenario of `file` and the what-ifs below it.
///
/// `host.load` runs once; each root starts from `host.root` over that base. Before a step with
/// what-ifs, the state is forked once per outcome, so the steps above run once. A `fails:` step
/// runs on a fork and the scenario continues from the state before it, so it keeps nothing.
/// After a failed step the rest of its scenario and the what-ifs of those later steps are
/// reported `NotRun`; what-ifs of the failed step itself still run.
pub async fn walk<H: ScenarioHost>(
    host: &H,
    file: &ScenarioFile,
    tree: &Tree,
    on_event: &(dyn Fn(Event) + Send + Sync),
) -> FileReport {
    let mut report = FileReport {
        path: file.path.clone(),
        status: Status::Green,
        error: None,
        scenarios: Vec::new(),
        unwritten: tree.unwritten(),
    };
    on_event(Event::FileStarted {
        path: file.path.clone(),
        total: tree.step_count(),
    });
    let base = match host.load(file).await {
        Ok(base) => base,
        Err(e) => {
            report.error = Some(e.to_string());
            report.settle();
            return report;
        }
    };
    for root in &tree.roots {
        let start = if root.scenario.skip.is_some() {
            Start::NotRun(NOT_STARTED)
        } else {
            let seed = seed(&file.path, &root.name);
            match host.root(&base, &root.scenario, seed).await {
                Ok(state) => Start::Ready(state),
                Err(e) => Start::Failed(e.to_string()),
            }
        };
        let mut stack = vec![Job {
            node: root,
            start,
            chain: Vec::new(),
        }];
        while let Some(job) = stack.pop() {
            let (scenario, branches) = run_scenario(host, job, on_event).await;
            report.scenarios.push(scenario);
            stack.extend(branches.into_iter().rev());
        }
    }
    report.settle();
    report
}

async fn run_scenario<'t, H: ScenarioHost>(
    host: &H,
    job: Job<'t, H::State>,
    on_event: &(dyn Fn(Event) + Send + Sync),
) -> (ScenarioReport, Vec<Job<'t, H::State>>) {
    let node = job.node;
    let name = label(&node.name, &job.chain);
    let mut report = ScenarioReport {
        scenario: node.name.clone(),
        from: job.chain.clone(),
        skip: node.scenario.skip.clone(),
        error: None,
        steps: Vec::new(),
    };
    let mut inner = job.chain;
    inner.insert(0, node.name.clone());
    if let Some(reason) = &node.scenario.skip {
        on_event(Event::ScenarioSkipped {
            scenario: name,
            reason: reason.clone(),
        });
        return (report, Vec::new());
    }
    let started = match job.start {
        Start::Ready(state) => Ok(state),
        Start::NotRun(reason) => Err(reason),
        Start::Failed(error) => {
            report.error = Some(error);
            Err(NOT_STARTED)
        }
    };
    let mut state = match started {
        Ok(state) => state,
        Err(reason) => {
            let report = unrun(node, report, reason);
            for step in &report.steps {
                finished(&name, step, on_event);
            }
            return (report, unrun_branches(node, &inner));
        }
    };
    on_event(Event::ScenarioStarted {
        scenario: name.clone(),
    });
    let mut branches = Vec::new();
    let mut failed = false;
    for planned in &node.steps {
        if failed {
            let step = not_run(&planned.step, EARLIER_STEP);
            finished(&name, &step, on_event);
            report.steps.push(step);
            branches.extend(
                targets(&planned.what_ifs).map(|t| job_for(t, &inner, Start::NotRun(PARENT_STEP))),
            );
            continue;
        }
        for target in targets(&planned.what_ifs) {
            let start = match target.scenario.skip {
                Some(_) => Start::NotRun(PARENT_STEP),
                None => match host.fork(&state).await {
                    Ok(forked) => Start::Ready(forked),
                    Err(e) => Start::Failed(format!("fork: {e}")),
                },
            };
            branches.push(job_for(target, &inner, start));
        }
        on_event(Event::StepStarted {
            scenario: name.clone(),
            step: planned.step.step.clone(),
        });
        let step = step::run(host, &mut state, &planned.step).await;
        failed = matches!(step.outcome, Outcome::Failed { .. });
        finished(&name, &step, on_event);
        report.steps.push(step);
    }
    (report, branches)
}

/// Every step reported, run or not, so a progress counter reaches the total.
fn finished(scenario: &str, step: &StepReport, on_event: &(dyn Fn(Event) + Send + Sync)) {
    on_event(Event::StepFinished {
        scenario: scenario.to_string(),
        step: step.clone(),
    });
}

fn targets(what_ifs: &[Branch]) -> impl Iterator<Item = &Node> {
    what_ifs.iter().filter_map(|b| match b {
        Branch::Scenario(node) => Some(node.as_ref()),
        _ => None,
    })
}

fn job_for<'t, S>(node: &'t Node, chain: &[String], start: Start<S>) -> Job<'t, S> {
    Job {
        node,
        start,
        chain: chain.to_vec(),
    }
}

fn unrun(node: &Node, mut report: ScenarioReport, reason: &str) -> ScenarioReport {
    report.steps = node
        .steps
        .iter()
        .map(|p| not_run(&p.step, reason))
        .collect();
    report
}

fn unrun_branches<'t, S>(node: &'t Node, chain: &[String]) -> Vec<Job<'t, S>> {
    node.steps
        .iter()
        .flat_map(|p| targets(&p.what_ifs))
        .map(|t| job_for(t, chain, Start::NotRun(PARENT_STEP)))
        .collect()
}

fn not_run(step: &ResolvedStep, reason: &str) -> StepReport {
    StepReport {
        step: step.step.clone(),
        who: step.session.as_ref().map(|s| s.label()),
        outcome: Outcome::NotRun {
            reason: reason.to_string(),
        },
        result: serde_json::Value::Null,
        writes: Vec::new(),
    }
}
