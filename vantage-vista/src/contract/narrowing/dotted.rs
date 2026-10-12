//! A condition on a dotted path (`root.leaf`) into a declared column either
//! errors or narrows; it is never accepted and then matched against nothing
//! or ignored.

use vantage_dataset::traits::ReadableValueSet;

use super::{Rows, UNMATCHABLE};
use crate::{CborValue, FilterOp, Vista, contract::Probe};

/// A leaf no fixture row holds, for a fixture with no nested values.
const PROBE_LEAF: &str = "contract_probe";

/// A `column.key == value` condition taken from one row's nested map.
struct Sample {
    /// The row the value came from; the condition must list it.
    id: String,
    path: String,
    value: CborValue,
    /// A row whose map lacks the entry; the condition must leave it out.
    other: Option<String>,
}

/// `listed` is the un-narrowed set, `None` when listing it failed, in which
/// case nothing can be compared and the probe is skipped.
///
/// When a row holds a map in a declared column, the condition built from
/// that map must, once accepted, list the row and leave out any row whose
/// map lacks the entry. Otherwise the path names a leaf no row holds, and an
/// accepted condition must still read.
pub(super) async fn dotted_path(vista: &Vista, listed: Option<&Rows>, probe: &mut Probe) {
    let Some(listed) = listed else {
        return;
    };
    let sample = nested_sample(vista, listed);
    let (path, value) = match &sample {
        Some(sample) => (sample.path.clone(), sample.value.clone()),
        None => {
            let Some(root) = vista.columns().keys().next() else {
                return;
            };
            let unmatchable = CborValue::Text(UNMATCHABLE.to_string());
            (format!("{root}.{PROBE_LEAF}"), unmatchable)
        }
    };
    // Rejecting the path is one of the two allowed answers.
    let Ok(narrowed) = vista.clone().with_condition(&path, FilterOp::Eq, value) else {
        return;
    };
    let rows = match narrowed.list_values().await {
        Ok(rows) => rows,
        Err(e) => {
            probe.fail(format!(
                "with_condition({path} == ..) is accepted but list_values fails: {e}"
            ));
            return;
        }
    };
    let Some(sample) = sample else {
        return;
    };
    if !rows.contains_key(&sample.id) {
        probe.fail(format!(
            "with_condition({path} == <row {}'s value>) is accepted but leaves out row {}: \
             a dotted path must narrow or be rejected, not match nothing",
            sample.id, sample.id
        ));
    }
    if let Some(other) = sample.other.filter(|other| rows.contains_key(other)) {
        probe.fail(format!(
            "with_condition({path} == <row {}'s value>) still lists row {other}, which lacks \
             that value: the dotted path is accepted but not applied",
            sample.id
        ));
    }
}

/// The first scalar entry of a map held in a declared column, plus a row
/// whose map in that column lacks the entry, if any.
fn nested_sample(vista: &Vista, rows: &Rows) -> Option<Sample> {
    let (id, column, key, value) = rows.iter().find_map(|(id, row)| {
        vista.columns().keys().find_map(|column| {
            let Some(CborValue::Map(entries)) = row.get(column) else {
                return None;
            };
            entries.iter().find_map(|(key, value)| {
                let scalar = !matches!(value, CborValue::Map(_) | CborValue::Array(_));
                match key {
                    CborValue::Text(key) if scalar && !key.contains('.') => {
                        Some((id, column, key, value))
                    }
                    _ => None,
                }
            })
        })
    })?;
    let entry = (CborValue::Text(key.clone()), value.clone());
    let other = rows
        .iter()
        .find(|(other, row)| {
            *other != id
                && !matches!(row.get(column), Some(CborValue::Map(entries)) if entries.contains(&entry))
        })
        .map(|(other, _)| other.clone());
    Some(Sample {
        id: id.clone(),
        path: format!("{column}.{key}"),
        value: value.clone(),
        other,
    })
}
