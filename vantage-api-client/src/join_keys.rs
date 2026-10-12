//! Lowering a YAML `references:` entry to the join keys of a Vista
//! `Reference`, shared by the REST and GraphQL factories.

use vantage_core::{Result, error};
use vantage_vista::{JoinKey, ReferenceKind, ReferenceSpec};

/// The join keys of `reference`: its `keys:` when given (each pair reads a
/// parent-row column and constrains a target column), else the single
/// `foreign_key` — on the parent for `has_one` (matched to the target's id),
/// on the target for `has_many` (matched to the parent's `source_id`).
pub(crate) fn spec_join_keys<R>(
    relation: &str,
    reference: &ReferenceSpec<R>,
    source_id: &str,
) -> Result<Vec<JoinKey>> {
    if !reference.keys.is_empty() {
        return Ok(reference.keys.clone());
    }
    let foreign_key = reference.foreign_key_or(relation);
    match reference.kind {
        ReferenceKind::HasOne => Ok(vec![JoinKey::to_target_id(foreign_key)]),
        ReferenceKind::HasMany => Ok(vec![JoinKey::new(source_id, foreign_key)]),
        _ => Err(error!(
            "references: entries must be has_one or has_many",
            relation = relation
        )),
    }
}
