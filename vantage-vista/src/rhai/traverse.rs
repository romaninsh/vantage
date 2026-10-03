//! The `ref(relation)` step: follow a relation from every row of a set.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use super::bridge::block_on;
use super::no_rows::NoRowsShell;
use super::read::fetch_capped;
use crate::{FilterOp, reference::ReferenceKind, vista::Vista};

/// Most rows a multi-row `ref` step will collect keys from.
const REF_ROW_CAP: usize = 1000;

/// Follow `rel` from the rows of `vista`, at most `limit` of them. One row
/// goes through [`Vista::get_ref`], keeping backend-specific traversal;
/// several rows narrow the bare target with an `in` condition on the join
/// column; no rows give the target as an empty set.
pub(crate) fn traverse(vista: &Vista, rel: &str, limit: Option<usize>) -> Result<Vista> {
    let cap = limit.map_or(REF_ROW_CAP + 1, |n| n.min(REF_ROW_CAP + 1));
    let rows = block_on(fetch_capped(vista, Some(cap)))??;
    if rows.len() > REF_ROW_CAP {
        return Err(error!("ref over too many rows", limit = REF_ROW_CAP));
    }
    if let [(_, row)] = rows.as_slice() {
        return vista.get_ref(rel, row);
    }
    let reference = vista
        .get_reference(rel)
        .ok_or_else(|| error!("No reference with this name", relation = rel))?;
    let mut target = vista.get_ref_target(rel)?;
    if rows.is_empty() {
        return Ok(NoRowsShell::wrap(target));
    }
    let (column, keys): (String, Vec<CborValue>) = match reference.kind {
        // Target rows point at ours: match their foreign key against our ids.
        ReferenceKind::HasMany => {
            let id_col = vista.get_id_column();
            let ids = rows
                .iter()
                .map(|(key, row)| {
                    id_col
                        .and_then(|c| row.get(c).cloned())
                        .unwrap_or_else(|| CborValue::Text(key.clone()))
                })
                .collect();
            (reference.foreign_key.clone(), ids)
        }
        // Our rows point at the target: match its id against our foreign keys.
        ReferenceKind::HasOne => {
            let id_col = target
                .get_id_column()
                .ok_or_else(|| error!("Reference target has no id column", relation = rel))?
                .to_string();
            let fks: Vec<CborValue> = rows
                .iter()
                .filter_map(|(_, row)| row.get(&reference.foreign_key).cloned())
                .filter(|v| !matches!(v, CborValue::Null))
                .collect();
            if fks.is_empty() {
                return Ok(NoRowsShell::wrap(target));
            }
            (id_col, fks)
        }
    };
    target.add_condition(column, FilterOp::InSet, CborValue::Array(keys))?;
    Ok(target)
}
