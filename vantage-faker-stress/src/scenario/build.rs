//! Turning a loaded scenario into engine inputs: durations, scaling,
//! `SimDef`s with vantage-ui's defaults, and `FakerColumn`s.

use std::time::Duration;

use vantage_faker::{FakerColumn, SimDef};

use super::Scenario;

/// `500ms`, `1.5s`, `2m`, `6h`, `3d` or bare seconds, as vantage-ui reads them.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let bad = || format!("`{s}` is not a duration (e.g. 500ms, 30s, 5m, 12h, 3d)");
    let (num, unit) = match s {
        _ if s.ends_with("ms") => (&s[..s.len() - 2], 0.001),
        _ if s.ends_with('s') => (&s[..s.len() - 1], 1.0),
        _ if s.ends_with('m') => (&s[..s.len() - 1], 60.0),
        _ if s.ends_with('h') => (&s[..s.len() - 1], 3600.0),
        _ if s.ends_with('d') => (&s[..s.len() - 1], 86_400.0),
        _ => (s, 1.0),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    Duration::try_from_secs_f64(n * unit).map_err(|_| bad())
}

/// `n × factor`, rounded; a non-zero `n` stays at least 1.
fn scale(n: usize, factor: f64) -> usize {
    if n == 0 {
        return 0;
    }
    ((n as f64 * factor).round() as usize).max(1)
}

impl Scenario {
    pub fn duration(&self) -> Result<Duration, String> {
        parse_duration(&self.stress.duration)
    }

    /// Multiply every `burst` and `max` by `sims` and every table `count` by `rows`.
    pub fn scaled(&self, sims: f64, rows: f64) -> Scenario {
        let mut s = self.clone();
        for spec in s.sims.values_mut() {
            let spawn = &mut spec.spawn;
            spawn.burst = Some(scale(spawn.burst.unwrap_or(1), sims));
            spawn.max = Some(scale(spawn.max.unwrap_or(1), sims));
        }
        for table in s.tables.values_mut() {
            table.count = scale(table.count, rows);
        }
        s
    }

    pub fn without_warm(&self) -> Scenario {
        let mut s = self.clone();
        for spec in s.sims.values_mut() {
            spec.warm = None;
        }
        s
    }

    /// Sum of every def's resolved `max`.
    pub fn max_total(&self) -> usize {
        self.sims.values().map(|s| s.spawn.max.unwrap_or(1)).sum()
    }

    /// One `SimDef` per sim, with vantage-ui's defaults: `burst: 1`,
    /// `rate: 0`, `max: 1`, `clock: 1`, and the first table when no `table:`.
    pub fn sim_defs(&self) -> Result<Vec<SimDef>, String> {
        let first = self.tables.keys().next().cloned();
        self.sims
            .iter()
            .map(|(name, spec)| {
                let table = spec
                    .table
                    .clone()
                    .or_else(|| first.clone())
                    .ok_or_else(|| format!("sim {name}: no table to write"))?;
                let spawn = &spec.spawn;
                let mut def = SimDef::new(name, table, spec.script.clone())
                    .with_spawn(
                        spawn.burst.unwrap_or(1),
                        spawn.rate.unwrap_or(0.0),
                        spawn.max.unwrap_or(1),
                    )
                    .with_clock(spec.clock.unwrap_or(1.0));
                if let Some(warm) = &spec.warm {
                    def = def.with_warm(
                        parse_duration(warm).map_err(|e| format!("sim {name}: warm: {e}"))?,
                    );
                }
                if !spawn.args.is_empty() {
                    let args = spawn.args.iter().map(|(k, v)| (k.clone(), v.clone()));
                    def = def.with_args(serde_json::Value::Object(args.collect()));
                }
                Ok(def)
            })
            .collect()
    }

    /// `id` first (unless declared), then the declared columns in order.
    pub fn faker_columns(&self, table: &str) -> Vec<FakerColumn> {
        let Some(spec) = self.tables.get(table) else {
            return Vec::new();
        };
        let mut cols = Vec::new();
        if !spec.columns.contains_key("id") {
            let mut id = FakerColumn::new("id", "string");
            id.flags.push("id".into());
            cols.push(id);
        }
        for (name, c) in &spec.columns {
            let mut col = FakerColumn::new(name, c.ty.as_deref().unwrap_or("string"));
            col.generator = c.faker.clone();
            cols.push(col);
        }
        cols
    }
}
