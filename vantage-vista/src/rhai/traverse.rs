//! The `ref(relation)` step: follow a relation from every row of a set.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::ReadableValueSet;

use super::bridge::block_on;
use crate::{FilterOp, reference::ReferenceKind, vista::Vista};

/// Most rows a multi-row `ref` step will collect keys from.
const REF_ROW_CAP: usize = 1000;

/// Follow `rel` from every row of `vista`. One row goes through
/// [`Vista::get_ref`], keeping backend-specific traversal; several rows narrow
/// the bare target with an `in` condition on the join column.
pub(crate) fn traverse(vista: &Vista, rel: &str) -> Result<Vista> {
    let rows = block_on(vista.list_values())??;
    if rows.len() > REF_ROW_CAP {
        return Err(error!(format!("ref over more than {REF_ROW_CAP} rows")));
    }
    if rows.len() == 1 {
        let (_, row) = rows.first().expect("one row");
        return vista.get_ref(rel, row);
    }
    let reference = vista
        .get_reference(rel)
        .ok_or_else(|| error!(format!("no reference named \"{rel}\"")))?;
    let mut target = vista.get_ref_target(rel)?;
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
                .ok_or_else(|| error!(format!("target of \"{rel}\" has no id column")))?
                .to_string();
            let fks = rows
                .values()
                .filter_map(|row| row.get(&reference.foreign_key).cloned())
                .filter(|v| !matches!(v, CborValue::Null))
                .collect();
            (id_col, fks)
        }
    };
    target.add_condition(column, FilterOp::InSet, CborValue::Array(keys))?;
    Ok(target)
}
