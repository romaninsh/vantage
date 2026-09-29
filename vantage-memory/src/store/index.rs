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

#[derive(Default)]
struct HashIndex {
    by_key: HashMap<String, IndexSet<String>>,
    /// Rows whose cell has no key; always candidates.
    unkeyed: IndexSet<String>,
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
            let cell = lookup(row, col).unwrap_or(&CborValue::Null);
            match key(cell) {
                Some(k) => {
                    ix.by_key.entry(k).or_default().insert(id.to_string());
                }
                None => {
                    ix.unkeyed.insert(id.to_string());
                }
            }
        }
    }

    pub fn remove(&mut self, id: &str, row: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            let cell = lookup(row, col).unwrap_or(&CborValue::Null);
            match key(cell) {
                Some(k) => {
                    if let Some(set) = ix.by_key.get_mut(&k) {
                        set.shift_remove(id);
                        if set.is_empty() {
                            ix.by_key.remove(&k);
                        }
                    }
                }
                None => {
                    ix.unkeyed.shift_remove(id);
                }
            }
        }
    }

    /// Candidate ids from the first top-level `Eq` / `InSet` condition on an
    /// indexed column, or `None` when no index applies. The caller still
    /// evaluates every condition on the candidates.
    pub fn candidates(&self, q: &Query) -> Option<IndexSet<String>> {
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
            let mut out: IndexSet<String> = ix.unkeyed.clone();
            for v in values {
                let k = key(v)?;
                if let Some(set) = ix.by_key.get(&k) {
                    out.extend(set.iter().cloned());
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
        assert_eq!(c.into_iter().collect::<Vec<_>>(), vec!["a".to_string()]);
        let q = Query::new().filter(MemoryCondition::cmp("other", FilterOp::Eq, CborValue::Null));
        assert!(ix.candidates(&q).is_none());
    }
}
