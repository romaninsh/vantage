//! Summaries, the JSON report, and the live table the CLI prints.

pub mod compare;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::sampler::Sample;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub peak_cpu_pct: f64,
    pub mean_cpu_pct: f64,
    pub peak_rss_mb: f64,
    pub mean_rss_mb: f64,
    pub peak_threads: f64,
    pub mean_threads: f64,
    pub peak_live: f64,
    pub mean_live: f64,
    pub peak_writes_per_s: f64,
    pub mean_writes_per_s: f64,
    pub peak_events_per_s: f64,
    pub mean_events_per_s: f64,
    pub peak_lag_ms: f64,
    pub mean_lag_ms: f64,
    pub lagged: u64,
    pub spawned: u64,
    pub ended: u64,
    pub errored: u64,
    /// CPU percent per live sim, over the second half of the run.
    pub cpu_per_sim: f64,
    /// Resident KB per live sim, over the second half of the run.
    pub rss_kb_per_sim: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    /// Ramp target in live sims (or rows); `None` for a plain run.
    pub target: Option<usize>,
    pub summary: Summary,
    pub samples: Vec<Sample>,
    pub warm_secs: Option<f64>,
    pub verdict: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub scenario: String,
    pub mode: String,
    pub faker_version: String,
    pub git_rev: String,
    pub profile: String,
    pub steps: Vec<Step>,
}

fn peak_mean(samples: &[Sample], f: impl Fn(&Sample) -> f64) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let peak = samples.iter().map(&f).fold(f64::MIN, f64::max);
    let mean = samples.iter().map(&f).sum::<f64>() / samples.len() as f64;
    (peak, mean)
}

pub fn summarize(samples: &[Sample]) -> Summary {
    let (peak_cpu_pct, mean_cpu_pct) = peak_mean(samples, |s| s.cpu_pct);
    let (peak_rss_mb, mean_rss_mb) = peak_mean(samples, |s| s.rss_mb);
    let (peak_threads, mean_threads) = peak_mean(samples, |s| s.threads as f64);
    let (peak_live, mean_live) = peak_mean(samples, |s| s.live as f64);
    let (peak_writes_per_s, mean_writes_per_s) = peak_mean(samples, |s| s.writes_per_s);
    let (peak_events_per_s, mean_events_per_s) = peak_mean(samples, |s| s.events_per_s);
    let (peak_lag_ms, mean_lag_ms) = peak_mean(samples, |s| s.lag_ms);
    let tail = &samples[samples.len() / 2..];
    let (_, tail_live) = peak_mean(tail, |s| s.live as f64);
    let (_, tail_cpu) = peak_mean(tail, |s| s.cpu_pct);
    let (_, tail_rss) = peak_mean(tail, |s| s.rss_mb);
    let per_sim = |v: f64| if tail_live > 0.0 { v / tail_live } else { 0.0 };
    let last = samples.last();
    Summary {
        peak_cpu_pct,
        mean_cpu_pct,
        peak_rss_mb,
        mean_rss_mb,
        peak_threads,
        mean_threads,
        peak_live,
        mean_live,
        peak_writes_per_s,
        mean_writes_per_s,
        peak_events_per_s,
        mean_events_per_s,
        peak_lag_ms,
        mean_lag_ms,
        lagged: last.map_or(0, |s| s.lagged),
        spawned: last.map_or(0, |s| s.spawned),
        ended: last.map_or(0, |s| s.ended),
        errored: last.map_or(0, |s| s.errored),
        cpu_per_sim: per_sim(tail_cpu),
        rss_kb_per_sim: per_sim(tail_rss * 1024.0),
    }
}

pub fn print_header() {
    println!(
        "{:>5} {:>6} {:>8} {:>7} {:>7} {:>7} {:>6} {:>7} {:>9} {:>9} {:>7} {:>7}",
        "t",
        "live",
        "spawned",
        "ended",
        "errored",
        "threads",
        "cpu%",
        "rss",
        "writes/s",
        "events/s",
        "lagged",
        "lag ms"
    );
}

pub fn print_row(s: &Sample) {
    println!(
        "{:>5.0} {:>6} {:>8} {:>7} {:>7} {:>7} {:>6.0} {:>7.1} {:>9.0} {:>9.0} {:>7} {:>7.0}",
        s.t,
        s.live,
        s.spawned,
        s.ended,
        s.errored,
        s.threads,
        s.cpu_pct,
        s.rss_mb,
        s.writes_per_s,
        s.events_per_s,
        s.lagged,
        s.lag_ms
    );
}

pub fn print_summary(step: &Step) {
    let s = &step.summary;
    if let Some(target) = step.target {
        println!("-- step {target}");
    }
    println!(
        "peak cpu {:.0}%  mean cpu {:.0}%  peak rss {:.1} MB  peak threads {:.0}  peak live {:.0}",
        s.peak_cpu_pct, s.mean_cpu_pct, s.peak_rss_mb, s.peak_threads, s.peak_live
    );
    println!(
        "mean writes/s {:.0}  mean events/s {:.0}  peak lag {:.0} ms  lagged {}",
        s.mean_writes_per_s, s.mean_events_per_s, s.peak_lag_ms, s.lagged
    );
    println!(
        "per sim: {:.3}% cpu, {:.0} KB rss   spawned {} ended {} errored {}",
        s.cpu_per_sim, s.rss_kb_per_sim, s.spawned, s.ended, s.errored
    );
    if let Some(w) = step.warm_secs {
        println!("warm start {w:.2} s");
    }
    if let Some(v) = &step.verdict {
        println!("verdict: {v}");
    }
    if let Some(r) = &step.stop_reason {
        println!("ramp stopped: {r}");
    }
}
