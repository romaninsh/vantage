//! Chaos verdicts: did the engine contain a misbehaving sim?

use crate::runner::RunOutput;
use crate::scenario::Expect;

/// CPU points above the idle baseline a settled run may still use.
const CPU_SLACK: f64 = 10.0;
/// Samples at the end of the run the CPU check averages.
const TAIL: usize = 3;

/// `Ok` when contained; otherwise every reason it was not.
pub fn verdict(expect: &Expect, out: &RunOutput) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    let tail = &out.samples[out.samples.len().saturating_sub(TAIL)..];
    if !tail.is_empty() {
        let cpu = tail.iter().map(|s| s.cpu_pct).sum::<f64>() / tail.len() as f64;
        if cpu > out.baseline_cpu + CPU_SLACK {
            why.push(format!(
                "cpu still {cpu:.0}% at the end (baseline {:.0}%)",
                out.baseline_cpu
            ));
        }
    }
    let errored = out.samples.last().map_or(0, |s| s.errored);
    if errored < expect.errored_min {
        why.push(format!(
            "{errored} sims errored, expected at least {}",
            expect.errored_min
        ));
    }
    if let Some(max) = expect.max_threads {
        let peak = out.samples.iter().map(|s| s.threads).max().unwrap_or(0);
        if peak > max {
            why.push(format!("threads peaked at {peak}, above {max}"));
        }
    }
    if out.panics > 0 {
        why.push(format!("{} panics", out.panics));
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

pub fn describe(result: &Result<(), Vec<String>>) -> String {
    match result {
        Ok(()) => "contained".into(),
        Err(why) => format!("NOT contained: {}", why.join("; ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunOutput;
    use crate::sampler::Sample;
    use crate::scenario::Expect;

    fn out(cpus: &[f64], errored: u64, threads: usize, panics: u64) -> RunOutput {
        RunOutput {
            samples: cpus
                .iter()
                .enumerate()
                .map(|(i, &c)| Sample {
                    t: i as f64,
                    live: 1,
                    spawned: 1,
                    ended: 0,
                    errored,
                    threads,
                    cpu_pct: c,
                    rss_mb: 10.0,
                    writes_per_s: 0.0,
                    events_per_s: 0.0,
                    lagged: 0,
                    lag_ms: 0.0,
                })
                .collect(),
            warm_secs: None,
            baseline_cpu: 2.0,
            panics,
        }
    }

    #[test]
    fn contained_when_cpu_settles_and_errors_seen() {
        let e = Expect {
            errored_min: 1,
            max_threads: Some(50),
        };
        assert!(verdict(&e, &out(&[100.0, 100.0, 5.0, 4.0, 3.0], 1, 40, 0)).is_ok());
    }

    #[test]
    fn pinned_cpu_is_not_contained() {
        let e = Expect {
            errored_min: 0,
            max_threads: None,
        };
        let why = verdict(&e, &out(&[100.0, 100.0, 100.0], 0, 10, 0)).unwrap_err();
        assert!(why[0].contains("cpu"), "{why:?}");
    }

    #[test]
    fn missing_errors_threads_and_panics_are_each_reported() {
        let e = Expect {
            errored_min: 2,
            max_threads: Some(20),
        };
        let why = verdict(&e, &out(&[1.0, 1.0, 1.0], 1, 30, 1)).unwrap_err();
        assert_eq!(why.len(), 3, "{why:?}");
    }
}
