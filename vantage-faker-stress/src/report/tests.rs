use super::*;
use crate::sampler::Sample;

fn sample(t: f64, live: usize, cpu: f64, rss: f64) -> Sample {
    Sample {
        t,
        live,
        spawned: live as u64,
        ended: 0,
        errored: 0,
        threads: live + 10,
        cpu_pct: cpu,
        rss_mb: rss,
        writes_per_s: 100.0,
        events_per_s: 90.0,
        lagged: 0,
        lag_ms: 5.0,
    }
}

#[test]
fn summary_takes_peaks_means_and_tail_cost_per_sim() {
    let s = summarize(&[
        sample(1.0, 100, 50.0, 100.0),
        sample(2.0, 100, 150.0, 200.0),
        sample(3.0, 200, 100.0, 300.0),
        sample(4.0, 200, 100.0, 300.0),
    ]);
    assert_eq!(s.peak_cpu_pct, 150.0);
    assert_eq!(s.mean_cpu_pct, 100.0);
    assert_eq!(s.peak_live, 200.0);
    // Tail half: samples 3 and 4, 100 % over 200 sims, 300 MB over 200 sims.
    assert!((s.cpu_per_sim - 0.5).abs() < 1e-9);
    assert!((s.rss_kb_per_sim - 1536.0).abs() < 1e-9);
}

#[test]
fn summary_of_nothing_is_zero() {
    let s = summarize(&[]);
    assert_eq!(s.peak_cpu_pct, 0.0);
    assert_eq!(s.cpu_per_sim, 0.0);
}

#[test]
fn compare_shows_both_sides_and_delta() {
    let step = |cpu| Step {
        target: None,
        summary: summarize(&[sample(1.0, 10, cpu, 50.0)]),
        samples: vec![],
        warm_secs: None,
        verdict: None,
        stop_reason: None,
    };
    let a = Report {
        scenario: "churn".into(),
        mode: "run".into(),
        faker_version: "0.7.0".into(),
        git_rev: "aaa".into(),
        profile: "debug".into(),
        steps: vec![step(100.0)],
    };
    let mut b = a.clone();
    b.faker_version = "0.8.0".into();
    b.steps = vec![step(50.0)];
    let text = compare::compare(&a, &b);
    assert!(text.contains("0.7.0") && text.contains("0.8.0"));
    assert!(text.contains("-50.0%"), "{text}");
}
