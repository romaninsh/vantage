//! Scenario files: tables, sims and stress settings, in the shape of a
//! vantage-ui faker datasource. `SimSpec` and `SpawnSpec` mirror
//! vantage-ui's `crates/inventory/src/faker_sims.rs`.

mod build;
mod include;
#[cfg(test)]
mod tests;

use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use vantage_faker::ColumnGen;

pub use build::parse_duration;

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
pub struct TableSpec {
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub columns: IndexMap<String, ColumnSpec>,
}

/// A column as a vantage-ui table declares it: a type and an optional generator.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub faker: Option<ColumnGen>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSpec {
    #[serde(default)]
    pub table: Option<String>,
    pub script: String,
    #[serde(default)]
    pub clock: Option<f64>,
    #[serde(default)]
    pub warm: Option<String>,
    #[serde(default)]
    pub spawn: SpawnSpec,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnSpec {
    #[serde(default)]
    pub burst: Option<usize>,
    #[serde(default)]
    pub rate: Option<f64>,
    #[serde(default)]
    pub max: Option<usize>,
    #[serde(default)]
    pub args: IndexMap<String, serde_json::Value>,
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
