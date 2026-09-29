//! Scenario files: a faker dataset (`seed`, `tables`, `sims`, as
//! vantage-faker's `DatasetSpec` reads them) plus `stress` settings.

mod build;
mod include;
#[cfg(test)]
mod tests;

use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use vantage_faker::config::{SimSpec, TableSpec};

pub use vantage_faker::config::parse_duration;

/// `DatasetSpec`'s fields spelled out rather than flattened: serde's
/// `flatten` does not combine with `deny_unknown_fields`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub tables: IndexMap<String, TableSpec>,
    #[serde(default)]
    pub sims: IndexMap<String, SimSpec>,
    #[serde(default)]
    pub stress: StressSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StressSpec {
    #[serde(default = "default_duration")]
    pub duration: String,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub ramp: RampSpec,
    #[serde(default)]
    pub expect: Option<Expect>,
    /// Attach a Dio to every table, as `--dio` does.
    #[serde(default)]
    pub dio: bool,
}

impl Default for StressSpec {
    fn default() -> Self {
        Self {
            duration: default_duration(),
            limits: Limits::default(),
            ramp: RampSpec::default(),
            expect: None,
            dio: false,
        }
    }
}

fn default_duration() -> String {
    "30s".into()
}

/// Thresholds a ramp stops at.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu_pct: Option<f64>,
    pub event_lag_ms: Option<f64>,
    pub rss_mb: Option<f64>,
}

/// What one ramp unit is. `base` is the unit count at scale 1 (default: the
/// sum of every def's `max`); `sims: false` ramps table rows only.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampSpec {
    #[serde(default)]
    pub base: Option<usize>,
    #[serde(default = "yes")]
    pub sims: bool,
}

impl Default for RampSpec {
    fn default() -> Self {
        Self {
            base: None,
            sims: true,
        }
    }
}

fn yes() -> bool {
    true
}

/// What a chaos scenario must show to count as contained.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub errored_min: u64,
    #[serde(default)]
    pub max_threads: Option<usize>,
}

/// Load `<root>/<name>/scenario.yaml`, resolving `!include`s relative to
/// its directory but never outside `root`.
pub fn load(root: &Path, name: &str) -> Result<Scenario, String> {
    let file = root.join(name).join("scenario.yaml");
    let label = format!("{name}/scenario.yaml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{label}: {e}"))?;
    let raw: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).map_err(|e| format!("{label}: {e}"))?;
    let resolved =
        include::resolve(raw, &root.join(name), root).map_err(|e| format!("{label}: {e}"))?;
    serde_yaml_ng::from_value(resolved).map_err(|e| format!("{label}: {e}"))
}
