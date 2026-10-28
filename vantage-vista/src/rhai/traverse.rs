//! The `ref(relation)` step: follow a relation from every row of a set.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use super::bridge::block_on;
use super::handle::RowWindow;
use super::no_rows::NoRowsShell;
use super::read::fetch_rows;
use crate::{FilterOp, unknown_relation, vista::Vista};

/// Most rows a multi-row `ref` step will collect keys from.
const REF_ROW_CAP: usize = 1000;

/// Follow `rel` from the rows of `vista` in `window`. One row goes through
/// [`Vista::get_ref`], keeping backend-specific traversal; several rows
/// narrow the bare target with an `in` condition on the join key; no rows
/// give the target as an empty set. Several rows need a join on one key.
pub(crate) fn traverse(vista: &Vista, rel: &str, window: RowWindow) -> Result<Vista> {
    let rows = block_on(fetch_rows(vista, window.capped(Some(REF_ROW_CAP + 1))))??;
    if rows.len() > REF_ROW_CAP {
        return Err(error!("ref over too many rows", limit = REF_ROW_CAP));
    }
    if let [(_, row)] = rows.as_slice() {
        return vista.get_ref(rel, Some(row));
    }
    let reference = vista.reference(rel).ok_or_else(|| unknown_relation(rel))?;
    let [key] = reference.keys.as_slice() else {
        return Err(error!(
            "ref over several rows needs a relation joined on one key",
            relation = rel
        ));
    };
    let target = vista.get_ref(rel, None)?;
    // A row's key under its id column may be missing from the row itself.
    let id_col = vista.id_column();
    let values: Vec<CborValue> = rows
        .iter()
        .filter_map(|(id, row)| match row.get(&key.from) {
            Some(v) => Some(v.clone()),
            None if id_col == Some(key.from.as_str()) => Some(CborValue::Text(id.clone())),
            None => None,
        })
        .filter(|v| !matches!(v, CborValue::Null))
        .collect();
    if values.is_empty() {
        return Ok(NoRowsShell::wrap(target));
    }
    let column = key.target_column(&target)?.to_string();
    target.with_condition(&column, FilterOp::InSet, CborValue::Array(values))
}
