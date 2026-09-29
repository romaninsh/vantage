//! `sims:` on a [`DatasetSpec`](super::DatasetSpec): one [`SimSpec`] per Rhai
//! sim, and the durations they parse their `warm:` from.

use std::time::Duration;

use indexmap::IndexMap;
use serde::Deserialize;

#[cfg(feature = "sim")]
use super::DatasetSpec;
#[cfg(feature = "sim")]
use crate::{SimDef, SimEngine};
#[cfg(feature = "sim")]
use vantage_memory::MemoryStore;

/// One `sims:` entry — a [`SimDef`](crate::SimDef) plan in YAML shape.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSpec {
    /// Default: the dataset's first declared table.
    #[serde(default)]
    pub table: Option<String>,
    /// Usually `!include ../sims/<name>.rhai`.
    pub script: String,
    /// Sim seconds per real second. Default 1.
    #[serde(default)]
    pub clock: Option<f64>,
    /// Start the spawner this much sim time ago (`90m`, `6h`, `3d`).
    #[serde(default)]
    pub warm: Option<String>,
    /// Rhai operations allowed between two sleeps. Default 5,000,000.
    #[serde(default)]
    pub ops: Option<u64>,
    #[serde(default)]
    pub spawn: SpawnSpec,
}

/// The spawner of one sim kind.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpawnSpec {
    /// Sims started at once. Default 1.
    pub burst: Option<usize>,
    /// New sims per sim minute after the burst. Default 0.
    pub rate: Option<f64>,
    /// Most sims of this kind alive at once. Default 1.
    pub max: Option<usize>,
    /// Handed to every spawned sim as `args`.
    pub args: IndexMap<String, serde_json::Value>,
}

#[cfg(feature = "sim")]
impl DatasetSpec {
    /// One [`SimDef`] per `sims:` entry, in declaration order, with
    /// defaults `burst: 1`, `rate: 0`, `max: 1`, `clock: 1`. A sim with no
    /// `table:` uses this dataset's first declared table; with no tables at
    /// all, a missing `table:` is an error.
    pub fn sim_defs(&self) -> Result<Vec<SimDef>, String> {
        let default_table = self.tables.keys().next();
        let mut defs = Vec::with_capacity(self.sims.len());
        for (name, spec) in &self.sims {
            let table = match &spec.table {
                Some(t) => t.clone(),
                None => default_table.cloned().ok_or_else(|| {
                    format!("sim {name}: no `table:`, and the dataset declares no tables")
                })?,
            };
            let mut def = SimDef::new(name.clone(), table, spec.script.clone())
                .with_spawn(
                    spec.spawn.burst.unwrap_or(1),
                    spec.spawn.rate.unwrap_or(0.0),
                    spec.spawn.max.unwrap_or(1),
                )
                .with_clock(spec.clock.unwrap_or(1.0));
            if let Some(warm) = &spec.warm {
                def = def.with_warm(parse_duration(warm)?);
            }
            if let Some(ops) = spec.ops {
                def = def.with_ops(ops);
            }
            if !spec.spawn.args.is_empty() {
                let args = spec.spawn.args.iter().map(|(k, v)| (k.clone(), v.clone()));
                def = def.with_args(serde_json::Value::Object(args.collect()));
            }
            defs.push(def);
        }
        Ok(defs)
    }

    /// Build and start the sim engine over `store`'s tables, seeded with
    /// [`DatasetSpec::seed`]. `None` when there are no `sims:`.
    pub fn start_sims(&self, store: &MemoryStore) -> Result<Option<SimEngine>, String> {
        if self.sims.is_empty() {
            return Ok(None);
        }
        let mut builder = SimEngine::builder().store(store);
        for def in self.sim_defs()? {
            builder = builder.sim(def);
        }
        if let Some(seed) = self.seed {
            builder = builder.seed(seed);
        }
        builder.start().map(Some)
    }
}

/// `500ms`, `1.5s`, `2m`, `6h`, `3d`, or bare seconds. Minutes, hours and
/// days are whole numbers.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let trimmed = s.trim();
    let parsed = if let Some(ms) = trimmed.strip_suffix("ms") {
        ms.trim().parse::<u64>().ok().map(Duration::from_millis)
    } else if let Some(sec) = trimmed.strip_suffix('s') {
        sec.trim()
            .parse::<f64>()
            .ok()
            .and_then(|s| Duration::try_from_secs_f64(s).ok())
    } else if let Some(min) = trimmed.strip_suffix('m') {
        min.trim()
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(60))
            .map(Duration::from_secs)
    } else if let Some(hours) = trimmed.strip_suffix('h') {
        hours
            .trim()
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(3_600))
            .map(Duration::from_secs)
    } else if let Some(days) = trimmed.strip_suffix('d') {
        days.trim()
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(86_400))
            .map(Duration::from_secs)
    } else {
        trimmed.parse::<u64>().ok().map(Duration::from_secs)
    };
    parsed
        .ok_or_else(|| format!("`{s}` is not a duration (e.g. `500ms`, `1.5s`, `2m`, `6h`, `3d`)"))
}
