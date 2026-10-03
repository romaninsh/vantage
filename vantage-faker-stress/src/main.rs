//! `faker-stress`: run, ramp, list and compare faker sim scenarios.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use vantage_faker_stress::report::{self, Report, Step};
use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::{self, parse_duration};
use vantage_faker_stress::{ramp, verdict};

#[derive(Parser)]
#[command(about = "Stress harness for vantage-faker sims")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the scenarios under scenarios/.
    List,
    /// One pass of a scenario.
    Run {
        scenario: String,
        #[arg(long)]
        duration: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        #[arg(long)]
        dio: bool,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// One fresh pass per step, until a limit or MAX_LIVE.
    Ramp {
        scenario: String,
        #[arg(long, value_delimiter = ',', default_values_t = [50, 100, 200, 400, 800])]
        steps: Vec<usize>,
        #[arg(long, default_value = "20s")]
        hold: String,
        #[arg(long)]
        dio: bool,
        #[arg(long)]
        warm: bool,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Two JSON reports side by side.
    Compare { a: PathBuf, b: PathBuf },
}

fn scenarios_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    if let Err(e) = dispatch(Cli::parse().cmd).await {
        eprintln!("faker-stress: {e}");
        std::process::exit(1);
    }
}

async fn dispatch(cmd: Cmd) -> Result<(), String> {
    match cmd {
        Cmd::List => {
            for name in list(&scenarios_root()) {
                println!("{name}");
            }
            Ok(())
        }
        Cmd::Run {
            scenario,
            duration,
            scale,
            dio,
            json,
        } => {
            let s = scenario::load(&scenarios_root(), &scenario)?.scaled(scale, scale);
            let duration = match duration {
                Some(d) => parse_duration(&d)?,
                None => s.duration()?,
            };
            let step = pass(&s, None, RunOpts { duration, dio }).await?;
            finish(&scenario, "run", vec![step], json)
        }
        Cmd::Ramp {
            scenario,
            steps,
            hold,
            dio,
            warm,
            json,
        } => {
            let base = scenario::load(&scenarios_root(), &scenario)?;
            let base = if warm { base } else { base.without_warm() };
            let hold = parse_duration(&hold)?;
            let mut done: Vec<Step> = Vec::new();
            for target in steps {
                if let Some(reason) = ramp::over_max_live(&base, target) {
                    println!("ramp stopped: {reason}");
                    if let Some(last) = done.last_mut() {
                        last.stop_reason = Some(reason);
                    }
                    break;
                }
                let (sims, rows) = ramp::factor(&base, target);
                let s = base.scaled(sims, rows);
                let mut step = pass(
                    &s,
                    Some(target),
                    RunOpts {
                        duration: hold,
                        dio,
                    },
                )
                .await?;
                let stop = ramp::breach(&base.stress.limits, &step.summary);
                step.stop_reason = stop.clone();
                done.push(step);
                if stop.is_some() {
                    break;
                }
            }
            finish(&scenario, "ramp", done, json)
        }
        Cmd::Compare { a, b } => {
            let read = |p: &Path| -> Result<Report, String> {
                let text =
                    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
            };
            print!("{}", report::compare::compare(&read(&a)?, &read(&b)?));
            Ok(())
        }
    }
}

async fn pass(
    s: &scenario::Scenario,
    target: Option<usize>,
    opts: RunOpts,
) -> Result<Step, String> {
    if let Some(t) = target {
        println!("== step {t}");
    }
    report::print_header();
    let out = run(s, &opts, report::print_row).await?;
    let verdict = s
        .stress
        .expect
        .as_ref()
        .map(|e| verdict::describe(&verdict::verdict(e, &out)));
    let step = Step {
        target,
        summary: report::summarize(&out.samples),
        samples: out.samples,
        warm_secs: out.warm_secs,
        verdict,
        stop_reason: None,
    };
    report::print_summary(&step);
    Ok(step)
}

fn finish(
    scenario: &str,
    mode: &str,
    steps: Vec<Step>,
    json: Option<PathBuf>,
) -> Result<(), String> {
    let Some(path) = json else { return Ok(()) };
    let report = Report {
        scenario: scenario.into(),
        mode: mode.into(),
        faker_version: faker_version(),
        git_rev: git_rev(),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into(),
        steps,
    };
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Scenario names: directories under `root` (one level of nesting, for
/// `chaos/*`) that hold a `scenario.yaml`.
fn list(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let dirs = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    };
    for dir in dirs(root) {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if dir.join("scenario.yaml").exists() {
            names.push(name);
        } else {
            for sub in dirs(&dir) {
                if sub.join("scenario.yaml").exists() {
                    names.push(format!(
                        "{name}/{}",
                        sub.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
    }
    names.sort();
    names
}

fn faker_version() -> String {
    include_str!("../../vantage-faker/Cargo.toml")
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default()
}

fn git_rev() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
