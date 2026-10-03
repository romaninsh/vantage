//! Ramp steps: how a target maps onto a scenario, and when to stop.

use vantage_faker::sim::MAX_LIVE;

use crate::report::Summary;
use crate::scenario::{Limits, Scenario};

/// `(sims, rows)` scale factors that bring `scenario` to `target` units.
pub fn factor(scenario: &Scenario, target: usize) -> (f64, f64) {
    let base = scenario
        .stress
        .ramp
        .base
        .unwrap_or_else(|| scenario.max_total())
        .max(1);
    let f = target as f64 / base as f64;
    if scenario.stress.ramp.sims {
        (f, f)
    } else {
        (1.0, f)
    }
}

/// Why `target` can't run: its scaled `max`es pass the engine's cap.
pub fn over_max_live(scenario: &Scenario, target: usize) -> Option<String> {
    let (sims, rows) = factor(scenario, target);
    let total = scenario.scaled(sims, rows).max_total();
    (total > MAX_LIVE)
        .then(|| format!("step {target} needs {total} sims, above MAX_LIVE {MAX_LIVE}"))
}

/// The first limit a step's summary broke.
pub fn breach(limits: &Limits, s: &Summary) -> Option<String> {
    if let Some(max) = limits.cpu_pct
        && s.mean_cpu_pct > max
    {
        return Some(format!("mean cpu {:.0}% above {max}%", s.mean_cpu_pct));
    }
    if let Some(max) = limits.event_lag_ms
        && s.peak_lag_ms > max
    {
        return Some(format!("peak lag {:.0} ms above {max} ms", s.peak_lag_ms));
    }
    if let Some(max) = limits.rss_mb
        && s.peak_rss_mb > max
    {
        return Some(format!("peak rss {:.0} MB above {max} MB", s.peak_rss_mb));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::summarize;
    use crate::scenario::{Limits, load};

    fn scenario(yaml: &str) -> Scenario {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("s")).unwrap();
        std::fs::write(root.path().join("s/scenario.yaml"), yaml).unwrap();
        load(root.path(), "s").unwrap()
    }

    #[test]
    fn factor_defaults_to_sum_of_max() {
        let s = scenario(
            "tables: { t: { count: 10 } }\nsims:\n  a: { script: x, spawn: { max: 30 } }\n  b: { script: x, spawn: { max: 70 } }\n",
        );
        assert_eq!(factor(&s, 200), (2.0, 2.0));
    }

    #[test]
    fn factor_can_ramp_rows_only() {
        let s = scenario(
            "tables: { t: { count: 1000 } }\nsims: { a: { script: x } }\nstress: { ramp: { base: 1000, sims: false } }\n",
        );
        assert_eq!(factor(&s, 500), (1.0, 0.5));
    }

    #[test]
    fn ramp_stops_before_max_live() {
        let s = scenario(
            "tables: { t: { count: 1 } }\nsims: { a: { script: x, spawn: { burst: 10, max: 10 } } }\n",
        );
        let (sims, rows) = factor(&s, 2000);
        assert!(s.scaled(sims, rows).max_total() > vantage_faker::sim::MAX_LIVE);
        assert!(over_max_live(&s, 2000).is_some());
        assert!(over_max_live(&s, 500).is_none());
    }

    #[test]
    fn breach_names_the_limit() {
        let limits = Limits {
            cpu_pct: Some(50.0),
            event_lag_ms: None,
            rss_mb: None,
        };
        let summary = summarize(&[]);
        assert!(breach(&limits, &summary).is_none());
        let mut hot = summary.clone();
        hot.mean_cpu_pct = 80.0;
        assert!(breach(&limits, &hot).unwrap().contains("cpu"));
    }
}
