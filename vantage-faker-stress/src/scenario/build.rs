//! Turning a loaded scenario into engine inputs: durations, scaling, the
//! faker `DatasetSpec`, `SimDef`s and each table's Vista metadata.

use std::time::Duration;

use vantage_faker::SimDef;
use vantage_faker::config::DatasetSpec;
use vantage_vista::{Column, VistaMetadata, flags};

use super::{Scenario, parse_duration};

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

    /// Multiply every `burst`, `rate` and `max` by `sims` and every table
    /// `count` by `rows`. `rate` is not rounded; an absent `rate` stays absent.
    pub fn scaled(&self, sims: f64, rows: f64) -> Scenario {
        let mut s = self.clone();
        for spec in s.sims.values_mut() {
            let spawn = &mut spec.spawn;
            spawn.burst = Some(scale(spawn.burst.unwrap_or(1), sims));
            spawn.rate = spawn.rate.map(|r| r * sims);
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

    pub fn dataset(&self) -> DatasetSpec {
        DatasetSpec {
            seed: self.seed,
            tables: self.tables.clone(),
            sims: self.sims.clone(),
        }
    }

    /// One `SimDef` per sim, with faker's defaults.
    pub fn sim_defs(&self) -> Result<Vec<SimDef>, String> {
        self.dataset().sim_defs()
    }

    /// The id column first (unless declared), then the declared columns in
    /// order; every column orderable.
    pub fn vista_metadata(&self, table: &str) -> VistaMetadata {
        let spec = self.tables.get(table);
        let id = spec
            .and_then(|s| s.id_column.clone())
            .unwrap_or_else(|| "id".into());
        let mut metadata = VistaMetadata::new().with_id_column(id.clone());
        let declared = spec.map(|s| &s.columns);
        if !declared.is_some_and(|c| c.contains_key(&id)) {
            metadata = metadata.with_column(Column::new(&id, "string"));
        }
        for (name, c) in declared.into_iter().flatten() {
            let ty = c.ty.as_deref().unwrap_or("string");
            metadata = metadata.with_column(Column::new(name, ty));
        }
        for column in metadata.columns.values_mut() {
            if column.name == id {
                column.flags.push(flags::ID.into());
            }
            column.flags.push(flags::ORDERABLE.into());
        }
        metadata
    }
}
