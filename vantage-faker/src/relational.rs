//! Static relational tables: reference columns filled from another table's
//! deterministic id space, optionally with a controlled number of children
//! per parent.
//!
//! A static faker table seeds row `seq` under the id [`seed_id`]`(seq)`, so a
//! table of `n` rows owns ids `seed_id(0..n)`. A child table's reference
//! column that draws from that range always names a real parent row, and a
//! drill-down filter on it finds matches.

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::FakerColumn;
use crate::generator::hash::{self, STREAM_FAN_OUT};
use crate::value_gen::ValueGen;

/// Stride through a parent pool. Prime, so it is coprime with any realistic
/// pool size: picks spread across parents and cover each once `count` reaches
/// the pool size.
const STRIDE: usize = 7919;

/// Id of the static row at `seq`: zero-padded to 20 digits, so string order
/// is seq order.
pub fn seed_id(seq: usize) -> String {
    format!("{seq:020}")
}

/// A column holding ids of another static table with `parent_count` rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub column: String,
    pub parent_count: usize,
}

/// Children per parent for one [`Reference`] column: every parent gets
/// between `min` and `max` (inclusive) children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FanOut {
    pub column: String,
    pub min: usize,
    pub max: usize,
}

/// Generate the rows of a static relational table as `(id, record)` pairs in
/// `seq` order, ids being [`seed_id`]`(seq)`.
///
/// Each [`Reference`] column is overwritten with a parent id; references with
/// `parent_count == 0` are left to the column's own generation.
///
/// - Without `fan_out`, `count` rows are generated and row `seq` points at
///   parent `(seq · 7919 + 1) mod parent_count` in every reference column.
/// - With `fan_out` on a reference column, parent `p` gets a deterministic
///   (per salt) child count in `[min, max]`, its children are contiguous in
///   `seq`, the row count is the sum and `count` is ignored. Other reference
///   columns still stride. A `fan_out` naming no reference column is ignored.
///
/// `values` supplies every other cell — pass
/// [`ValueGen::from_seed`] for a reproducible table. It is told the final row
/// count, so trees and even-spread dates scale to the table.
pub fn relational_rows(
    values: &ValueGen,
    columns: &[FakerColumn],
    id_column: &str,
    count: usize,
    refs: &[Reference],
    fan_out: Option<&FanOut>,
) -> Vec<(String, Record<CborValue>)> {
    let refs: Vec<&Reference> = refs.iter().filter(|r| r.parent_count > 0).collect();
    let fan = fan_out.and_then(|f| {
        let parents = refs.iter().find(|r| r.column == f.column)?.parent_count;
        Some((f.column.as_str(), owners(values, f, parents)))
    });
    let total = fan.as_ref().map_or(count, |(_, owners)| owners.len());

    let values = values.clone().with_rows(total);
    (0..total)
        .map(|seq| {
            let id = seed_id(seq);
            let mut record = values.record_at(columns, id_column, &id, seq);
            for r in &refs {
                let parent = match &fan {
                    Some((column, owners)) if *column == r.column => owners[seq],
                    _ => (seq * STRIDE + 1) % r.parent_count,
                };
                record.insert(r.column.clone(), CborValue::Text(seed_id(parent)));
            }
            (id, record)
        })
        .collect()
}

/// Parent seq of each child row, children of one parent contiguous.
fn owners(values: &ValueGen, fan: &FanOut, parents: usize) -> Vec<usize> {
    let (lo, hi) = (fan.min.min(fan.max), fan.min.max(fan.max));
    let span = (hi - lo + 1) as u64;
    let salt = values.column_salt(&fan.column);
    (0..parents)
        .flat_map(|p| {
            let n = lo + (hash::mix(salt, p as u64, STREAM_FAN_OUT) % span) as usize;
            std::iter::repeat_n(p, n)
        })
        .collect()
}

#[cfg(test)]
mod tests;
