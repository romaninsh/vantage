//! Hash indexes on declared columns. `Eq` / `InSet` conditions AND-ed at the
//! top level of a query take their candidate rows from here.

use std::collections::HashMap;

use ciborium::Value as CborValue;
use indexmap::IndexSet;
use vantage_types::Record;
use vantage_vista::FilterOp;

use crate::eval::compare::lookup;
use crate::eval::{MemoryCondition, Query};

/// The largest magnitude an `i128` and an `f64` can both represent exactly,
/// so an integer key and a float key agree whenever `values_eq` (which
/// compares integers and floats as `f64`) would call them equal.
const MAX_EXACT: i128 = 1 << 53;

/// A hashable form of a cell. Integers and integral floats share a key so
/// `1` and `1.0` meet; a value outside `MAX_EXACT` keys by its `f64` bit
/// pattern instead, since that is what `values_eq` compares by at that
/// magnitude. This can key unequal huge integers alike (a false-positive
/// candidate that `matches_all` filters out) but never keys equal values
/// apart. Maps, arrays and bytes have no key.
fn key(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Null => Some("null".into()),
        CborValue::Bool(b) => Some(format!("b:{b}")),
        CborValue::Integer(i) => {
            let n = i128::from(*i);
            if n.unsigned_abs() <= MAX_EXACT as u128 {
                Some(format!("n:{n}"))
            } else {
                Some(format!("f:{}", (n as f64).to_bits()))
            }
        }
        CborValue::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() <= MAX_EXACT as f64 => {
            Some(format!("n:{}", *f as i128))
        }
        CborValue::Float(f) if *f == 0.0 => Some("n:0".into()),
        CborValue::Float(f) => Some(format!("f:{}", f.to_bits())),
        CborValue::Text(s) => Some(format!("s:{s}")),
        _ => None,
    }
}

fn cell_key(row: &Record<CborValue>, column: &str) -> Option<String> {
    key(lookup(row, column).unwrap_or(&CborValue::Null))
}

/// Bucket order is not kept (removal swaps); callers restore row order by
/// sorting candidates on their position in the row map.
#[derive(Default)]
struct HashIndex {
    by_key: HashMap<String, IndexSet<String>>,
    /// Rows whose cell has no key; always candidates.
    unkeyed: IndexSet<String>,
}

impl HashIndex {
    fn add(&mut self, k: Option<String>, id: &str) {
        let bucket = match k {
            Some(k) => self.by_key.entry(k).or_default(),
            None => &mut self.unkeyed,
        };
        bucket.insert(id.to_string());
    }

    fn remove(&mut self, k: Option<String>, id: &str) {
        match k {
            Some(k) => {
                if let Some(set) = self.by_key.get_mut(&k) {
                    set.swap_remove(id);
                    if set.is_empty() {
                        self.by_key.remove(&k);
                    }
                }
            }
            None => {
                self.unkeyed.swap_remove(id);
            }
        }
    }
}

pub(crate) struct Indexes {
    columns: HashMap<String, HashIndex>,
}

impl Indexes {
    pub fn new(columns: &[String]) -> Self {
        Self {
            columns: columns
                .iter()
                .map(|c| (c.clone(), HashIndex::default()))
                .collect(),
        }
    }

    pub fn add(&mut self, id: &str, row: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            ix.add(cell_key(row, col), id);
        }
    }

    pub fn remove(&mut self, id: &str, row: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            ix.remove(cell_key(row, col), id);
        }
    }

    /// Move row `id` from `old`'s buckets to `new`'s, skipping every column
    /// whose cell keys the same in both.
    pub fn update(&mut self, id: &str, old: &Record<CborValue>, new: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            let (was, is) = (cell_key(old, col), cell_key(new, col));
            if was != is {
                ix.remove(was, id);
                ix.add(is, id);
            }
        }
    }

    pub fn has(&self, column: &str) -> bool {
        self.columns.contains_key(column)
    }

    /// Index `column` over `rows`. A column already indexed is left alone.
    pub fn add_column<'a>(
        &mut self,
        column: &str,
        rows: impl IntoIterator<Item = (&'a String, &'a Record<CborValue>)>,
    ) {
        if self.has(column) {
            return;
        }
        let mut ix = HashIndex::default();
        for (id, row) in rows {
            ix.add(cell_key(row, column), id);
        }
        self.columns.insert(column.to_string(), ix);
    }

    /// Candidate ids from the first top-level `Eq` / `InSet` condition on an
    /// indexed column, or `None` when no index applies. Ids are unordered and
    /// may repeat; the caller still evaluates every condition on them.
    pub fn candidates(&self, q: &Query) -> Option<Vec<&str>> {
        q.conditions.iter().find_map(|c| {
            let MemoryCondition::Cmp { path, op, value } = c else {
                return None;
            };
            let ix = self.columns.get(path)?;
            let values: Vec<&CborValue> = match (op, value) {
                (FilterOp::Eq, v) => vec![v],
                (FilterOp::InSet, CborValue::Array(items)) => items.iter().collect(),
                _ => return None,
            };
            let mut out: Vec<&str> = ix.unkeyed.iter().map(String::as_str).collect();
            for v in values {
                let k = key(v)?;
                if let Some(set) = ix.by_key.get(&k) {
                    out.extend(set.iter().map(String::as_str));
                }
            }
            Some(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{MemoryCondition, Query};
    use vantage_vista::FilterOp;

    #[test]
    fn candidates_come_from_the_index() {
        let mut ix = Indexes::new(&["status".to_string()]);
        let row: Record<CborValue> = [("status".to_string(), CborValue::Text("Open".into()))]
            .into_iter()
            .collect();
        ix.add("a", &row);
        let q = Query::new().filter(MemoryCondition::cmp(
            "status",
            FilterOp::Eq,
            CborValue::Text("Open".into()),
        ));
        let c = ix.candidates(&q).unwrap();
        assert_eq!(c, vec!["a"]);
        let q = Query::new().filter(MemoryCondition::cmp("other", FilterOp::Eq, CborValue::Null));
        assert!(ix.candidates(&q).is_none());
    }
}
