//! `query` and `count`: candidate rows come from the hash index when a
//! top-level condition targets an indexed column, else a full scan.

use vantage_core::Result;

use crate::eval::{Query, matches_all, sort_rows};

use super::{MemoryTable, Row};

impl MemoryTable {
    /// Rows matching `q`, ordered and windowed.
    pub fn query(&self, q: &Query) -> Result<Vec<(String, Row)>> {
        let mut out = self.matching(q)?;
        sort_rows(&mut out, &q.order);
        let end = q
            .limit
            .map_or(out.len(), |l| q.offset.saturating_add(l).min(out.len()));
        let start = q.offset.min(end);
        Ok(out.drain(start..end).collect())
    }

    /// How many rows match `q`'s conditions and search (order and window ignored).
    pub fn count(&self, q: &Query) -> Result<usize> {
        Ok(self.matching(q)?.len())
    }

    /// Rows passing every condition and the search, in insertion order.
    fn matching(&self, q: &Query) -> Result<Vec<(String, Row)>> {
        let rows = self.rows.read();
        let mut out = Vec::new();
        match rows.indexes.candidates(q) {
            Some(ids) => {
                let mut candidates: Vec<(usize, &String)> = ids
                    .iter()
                    .filter_map(|id| rows.map.get_index_of(id).map(|pos| (pos, id)))
                    .collect();
                candidates.sort_by_key(|(pos, _)| *pos);
                for (_, id) in candidates {
                    let row = rows.map.get(id).expect("id came from rows.map");
                    if matches_all(q, row)? {
                        out.push((id.clone(), row.clone()));
                    }
                }
            }
            None => {
                for (id, row) in rows.map.iter() {
                    if matches_all(q, row)? {
                        out.push((id.clone(), row.clone()));
                    }
                }
            }
        }
        Ok(out)
    }
}
