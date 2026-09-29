//! YAML-deserializable config for a faker datasource: the tables to seed and
//! the sims to run over them, shared by vantage-ui, the stress harness and
//! the examples.
//!
//! [`DatasetSpec`] mirrors [`DatasetGen`]/[`TableGen`] one-for-one, plus
//! [`SimSpec`](sims::SimSpec) for the `sim` feature. Every struct rejects
//! unknown keys and fills the rest from vantage-ui's own defaults. The
//! structs are `#[non_exhaustive]`: build them with `Default` (or
//! [`DatasetSpec::new`]) and assign fields.

mod sims;
#[cfg(test)]
mod tests;

use indexmap::IndexMap;
use serde::Deserialize;
use vantage_core::Result;
use vantage_memory::MemoryStore;

use crate::{ColumnGen, DatasetGen, ExtraFields, FakerColumn, FanOut, TableGen};

pub use sims::{SimSpec, SpawnSpec, parse_duration};

/// A whole datasource's worth of tables and sims, as `!include`d or inline
/// YAML. Each caller resolves its own `!include`s before deserializing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DatasetSpec {
    /// Reproducible when `Some`; fresh entropy per table with `None`.
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub tables: IndexMap<String, TableSpec>,
    /// Needs the `sim` feature to run; always deserializes.
    #[serde(default)]
    pub sims: IndexMap<String, SimSpec>,
}

/// One `tables:` entry — a [`TableGen`] plan in YAML shape.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TableSpec {
    /// Default `"id"`.
    #[serde(default)]
    pub id_column: Option<String>,
    /// Ignored when `fan_out` is set on a reference column. Default 0.
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub columns: IndexMap<String, ColumnSpec>,
    #[serde(default)]
    pub indexed: Vec<String>,
    /// Column → target table, resolved in generation order.
    #[serde(default)]
    pub references: IndexMap<String, String>,
    #[serde(default)]
    pub fan_out: Option<FanOutSpec>,
    /// See [`TableGen::weirdness`]. Default 0.
    #[serde(default)]
    pub weirdness: Option<f64>,
    /// `{ count, size }` — see [`ExtraFields`]. Default none.
    #[serde(default)]
    pub extra_fields: Option<ExtraFields>,
}

/// One column's declared type and optional explicit generator.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ColumnSpec {
    /// Default `"string"`.
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub faker: Option<ColumnGen>,
}

/// Children per parent on a `references` column, in `[min, max]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct FanOutSpec {
    pub column: String,
    pub min: usize,
    pub max: usize,
}

impl DatasetSpec {
    /// A spec from its three parts.
    pub fn new(
        seed: Option<u64>,
        tables: IndexMap<String, TableSpec>,
        sims: IndexMap<String, SimSpec>,
    ) -> Self {
        Self { seed, tables, sims }
    }

    /// Seed every declared table into `store`, in reference order. See
    /// [`DatasetGen::generate`] for the exact seeding rules and errors.
    pub fn generate(&self, store: &MemoryStore) -> Result<()> {
        let mut dataset = DatasetGen::new(self.seed);
        for (name, table) in &self.tables {
            dataset = dataset.table(table_gen(name, table));
        }
        dataset.generate(store)?;
        Ok(())
    }
}

fn table_gen(name: &str, spec: &TableSpec) -> TableGen {
    let mut table = TableGen::new(name)
        .count(spec.count)
        .indexed(spec.indexed.clone());
    if let Some(id_column) = &spec.id_column {
        table = table.id_column(id_column.clone());
    }
    for (column, target) in &spec.references {
        table = table.reference(column.clone(), target.clone());
    }
    if let Some(fan_out) = &spec.fan_out {
        table = table.fan_out(FanOut {
            column: fan_out.column.clone(),
            min: fan_out.min,
            max: fan_out.max,
        });
    }
    if let Some(weirdness) = spec.weirdness {
        table = table.weirdness(weirdness);
    }
    if let Some(extra) = spec.extra_fields {
        table = table.extra_fields(extra);
    }
    for (name, column) in &spec.columns {
        let ty = column.ty.as_deref().unwrap_or("string");
        let mut col = FakerColumn::new(name.clone(), ty);
        if let Some(generator) = &column.faker {
            col = col.with_generator(generator.clone());
        }
        table = table.column(col);
    }
    table
}
