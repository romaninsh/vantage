//! Two reports side by side, step by step, with the change from a to b.

use std::fmt::Write as _;

use super::{Report, Summary};

/// A row label and the summary value it shows.
type Row = (&'static str, fn(&Summary) -> f64);

const ROWS: &[Row] = &[
    ("mean cpu%", |s| s.mean_cpu_pct),
    ("peak rss MB", |s| s.peak_rss_mb),
    ("peak threads", |s| s.peak_threads),
    ("mean live", |s| s.mean_live),
    ("mean writes/s", |s| s.mean_writes_per_s),
    ("mean events/s", |s| s.mean_events_per_s),
    ("peak lag ms", |s| s.peak_lag_ms),
    ("cpu% per sim", |s| s.cpu_per_sim),
    ("rss KB per sim", |s| s.rss_kb_per_sim),
];

fn delta(a: f64, b: f64) -> String {
    if a == 0.0 {
        return "-".into();
    }
    format!("{:+.1}%", (b - a) / a * 100.0)
}

pub fn compare(a: &Report, b: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} ({})", a.scenario, a.mode);
    let _ = writeln!(
        out,
        "{:<16} {:>14} {:>14} {:>9}",
        "",
        format!("{} {}", a.faker_version, a.git_rev),
        format!("{} {}", b.faker_version, b.git_rev),
        "change"
    );
    for (sa, sb) in a.steps.iter().zip(&b.steps) {
        if let Some(t) = sa.target {
            let _ = writeln!(out, "-- step {t}");
        }
        for (label, f) in ROWS {
            let (x, y) = (f(&sa.summary), f(&sb.summary));
            let _ = writeln!(out, "{label:<16} {x:>14.2} {y:>14.2} {:>9}", delta(x, y));
        }
    }
    if a.steps.len() != b.steps.len() {
        let _ = writeln!(
            out,
            "(step counts differ: {} vs {})",
            a.steps.len(),
            b.steps.len()
        );
    }
    out
}
