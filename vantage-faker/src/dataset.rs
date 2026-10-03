//! Reproducible seeding: [`DatasetGen`] generates each declared
//! [`TableGen`]'s rows into a [`MemoryStore`], referenced tables before the
//! tables that reference them, quietly so no subscriber sees the seed rows
//! trickle in one at a time.

mod extra;
mod order;
mod table;
#[cfg(test)]
mod tests;
mod validate;

use std::collections::HashMap;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_memory::{MemoryStore, MemoryTableHandle, TableDef};
use vantage_types::Record;

use crate::generator::hash;
use crate::relational::{Reference, check_plan};
pub use extra::ExtraFields;
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
    /// The whole plan is checked and every table's rows are generated
    /// before any table is touched, so an error leaves `store` as it was.
    /// Errors: a table name declared twice, an invalid column generator or
    /// fan-out, a fan-out on a non-reference column or over a parent that
    /// generated no rows, a reference to an undeclared table, a reference
    /// cycle, or a table already in `store` with another id column.
    ///
    /// A reference column draws from the rows its target generated in this
    /// call; rows the target table held before are never picked. Each table
    /// is then defined in `store` (with its `indexed` columns indexed, even
    /// if it existed), seeded quietly and unquieted — a single `Reset`
    /// follows, since no one is watching a table mid-seed.
    pub fn generate(&self, store: &MemoryStore) -> Result<Vec<MemoryTableHandle>> {
        let order = validate::check(&self.tables, store)?;

        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut planned = Vec::with_capacity(order.len());
        for i in order {
            let plan = &self.tables[i];
            let refs: Vec<Reference> = plan
                .refs
                .iter()
                .map(|r| Reference {
                    column: r.column.clone(),
                    parent_count: counts.get(r.target.as_str()).copied().unwrap_or(0),
                })
                .collect();
            check_plan(&refs, plan.fan_out.as_ref()).map_err(|reason| {
                error!("Table plan is invalid", table = plan.name, reason = reason)
            })?;
            let rows = plan.rows(seed_for(self.seed, &plan.name), &refs);
            counts.insert(&plan.name, rows.len());
            planned.push((plan, rows));
        }

        Ok(planned
            .into_iter()
            .map(|(plan, rows)| seed_table(store, plan, rows))
            .collect())
    }
}

/// Define `plan`'s table in `store` and write `rows` into it quietly.
fn seed_table(
    store: &MemoryStore,
    plan: &TableGen,
    rows: Vec<(String, Record<CborValue>)>,
) -> MemoryTableHandle {
    let table = store.define(
        &plan.name,
        TableDef {
            id_column: plan.id_column.clone(),
            ..TableDef::default()
        },
    );
    for column in &plan.indexed {
        table.add_index(column);
    }
    table.set_quiet(true);
    for (id, record) in rows {
        table.upsert(&id, record);
    }
    table.set_quiet(false);
    table
}

/// Fold `name` into `seed` (FNV-1a over its bytes, XORed in) so every table
/// draws an independent but reproducible stream from one dataset seed.
/// `None` stays `None` — each run gets fresh values.
fn seed_for(seed: Option<u64>, name: &str) -> Option<u64> {
    seed.map(|s| s ^ hash::fnv1a(name.as_bytes()))
}
