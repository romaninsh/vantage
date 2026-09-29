//! Reproducible seeding: [`DatasetGen`] generates each declared
//! [`TableGen`](table::TableGen)'s rows into a [`MemoryStore`], referenced
//! tables before the tables that reference them, quietly so no subscriber
//! sees the seed rows trickle in one at a time.

mod order;
pub mod table;
#[cfg(test)]
mod tests;

use vantage_memory::{MemoryStore, MemoryTableHandle, TableDef};

use crate::generator::hash;
use crate::relational::{Reference, check_plan, relational_rows};
use crate::value_gen::ValueGen;
pub use table::TableGen;

/// A set of tables to generate together, in reference order, from one
/// optional seed.
#[derive(Default)]
pub struct DatasetGen {
    seed: Option<u64>,
    tables: Vec<TableGen>,
}

impl DatasetGen {
    /// A dataset drawing from `seed` — reproducible when `Some`, fresh
    /// entropy per table when `None`.
    pub fn new(seed: Option<u64>) -> Self {
        Self {
            seed,
            tables: Vec::new(),
        }
    }

    pub fn table(mut self, table: TableGen) -> Self {
        self.tables.push(table);
        self
    }

    /// Generate every declared table into `store` and return the created
    /// (or pre-existing) tables in generation order.
    ///
    /// Each table's columns are validated, then its reference columns are
    /// checked against the already-generated row count of the table they
    /// point at. The table is defined in `store`, seeded quietly with
    /// [`relational_rows`], then unquieted — a single `Reset` follows, since
    /// no one is watching a table mid-seed.
    pub fn generate(&self, store: &MemoryStore) -> Result<Vec<MemoryTableHandle>, String> {
        let order = order::generation_order(&self.tables)?;
        let mut generated = Vec::with_capacity(order.len());

        for i in order {
            let plan = &self.tables[i];
            for col in &plan.columns {
                if let Some(generator) = &col.generator {
                    generator
                        .validate()
                        .map_err(|e| format!("table {}: column {}: {e}", plan.name, col.name))?;
                }
            }

            let refs: Vec<Reference> = plan
                .refs
                .iter()
                .map(|r| Reference {
                    column: r.column.clone(),
                    parent_count: store.table(&r.target).len(),
                })
                .collect();
            check_plan(&refs, plan.fan_out.as_ref())
                .map_err(|e| format!("table {}: {e}", plan.name))?;

            let table = store.define(
                &plan.name,
                TableDef {
                    id_column: plan.id_column.clone(),
                    indexed: plan.indexed.clone(),
                    id_prefix: None,
                },
            );
            if table.id_column() != plan.id_column.as_str() {
                return Err(format!(
                    "table {}: existing table's id column is `{}`, not `{}`",
                    plan.name,
                    table.id_column(),
                    plan.id_column
                ));
            }

            table.set_quiet(true);
            let values = ValueGen::from_seed(seed_for(self.seed, &plan.name));
            let rows = relational_rows(
                &values,
                &plan.columns,
                &plan.id_column,
                plan.count,
                &refs,
                plan.fan_out.as_ref(),
            );
            for (id, record) in rows {
                table.upsert(&id, record);
            }
            table.set_quiet(false);

            generated.push(table);
        }
        Ok(generated)
    }
}

/// Fold `name` into `seed` (FNV-1a over its bytes, XORed in) so every table
/// draws an independent but reproducible stream from one dataset seed.
/// `None` stays `None` — each run gets fresh values.
fn seed_for(seed: Option<u64>, name: &str) -> Option<u64> {
    seed.map(|s| s ^ hash::fnv1a(name.as_bytes()))
}
