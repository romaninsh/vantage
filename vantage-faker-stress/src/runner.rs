//! One pass of a scenario: build its tables, attach consumers, take an
//! idle CPU baseline, start the engine (timing any warm start), sample once
//! a second for the run's duration, then stop.

use std::time::{Duration, Instant};

use vantage_faker::{FakerHandle, FakerTable, SimEngine, StaticEffect};

use crate::load::{self, TableLoad};
use crate::panics;
use crate::sampler::{Sample, Sampler};
use crate::scenario::Scenario;

const TICK: Duration = Duration::from_secs(1);

pub struct RunOpts {
    pub duration: Duration,
    pub dio: bool,
}

pub struct RunOutput {
    pub samples: Vec<Sample>,
    /// Seconds `start()` spent in the warm start; `None` when no def warms.
    pub warm_secs: Option<f64>,
    /// CPU percent over the idle second before the engine started.
    pub baseline_cpu: f64,
    /// Panics anywhere in the process during the run.
    pub panics: u64,
}

pub async fn run(
    scenario: &Scenario,
    opts: &RunOpts,
    mut on_sample: impl FnMut(&Sample),
) -> Result<RunOutput, String> {
    panics::install();
    let panics_before = panics::count();
    let defs = scenario.sim_defs()?;
    let warms = defs.iter().any(|d| d.warm.is_some());

    let cache = tempfile::tempdir().map_err(|e| format!("cache dir: {e}"))?;
    let lens = if opts.dio || scenario.stress.dio {
        Some(load::lens(cache.path())?)
    } else {
        None
    };

    let mut handles: Vec<(String, FakerHandle)> = Vec::new();
    let mut loads: Vec<TableLoad> = Vec::new();
    for (name, spec) in &scenario.tables {
        let table = FakerTable::build(
            name.clone(),
            scenario.faker_columns(name),
            "id",
            Box::new(StaticEffect { count: spec.count }),
        );
        let (vista, handle) = table.split();
        loads.push(load::attach(vista, &handle, lens.as_ref()).await?);
        handles.push((name.clone(), handle));
    }

    let mut sampler = Sampler::new();
    tokio::time::sleep(TICK).await;
    let baseline_cpu = sampler.cpu_now();

    let mut builder = SimEngine::builder();
    for (name, handle) in &handles {
        builder = builder.table(name.clone(), handle.ctx());
    }
    for def in defs {
        builder = builder.sim(def);
    }
    if let Some(seed) = scenario.seed {
        builder = builder.seed(seed);
    }
    let started = Instant::now();
    let engine = tokio::task::spawn_blocking(move || builder.start())
        .await
        .map_err(|e| format!("engine start panicked: {e}"))??;
    let warm_secs = warms.then(|| started.elapsed().as_secs_f64());

    let mut samples = Vec::new();
    let live_from = Instant::now();
    let mut ticker = tokio::time::interval(TICK);
    ticker.tick().await;
    while live_from.elapsed() < opts.duration {
        ticker.tick().await;
        let sample = sampler.sample(engine.stats(), load::sum(&loads));
        on_sample(&sample);
        samples.push(sample);
    }

    tokio::task::spawn_blocking(move || engine.stop())
        .await
        .map_err(|e| format!("engine stop panicked: {e}"))?;
    drop(loads);
    drop(handles);
    Ok(RunOutput {
        samples,
        warm_secs,
        baseline_cpu,
        panics: panics::count() - panics_before,
    })
}
