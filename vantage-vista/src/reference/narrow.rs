//! [`Reference::narrow`]: narrowing a join's target to one parent row.

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, VantageError, error};
use vantage_types::Record;

use super::{JoinKey, Reference};
use crate::filter::FilterOp;
use crate::vista::Vista;

#[cfg(test)]
mod tests;

impl Reference {
    /// Narrow this join's bare `target` to the rows related to `row`.
    ///
    /// Each key reads the parent's `from` column and constrains the target's
    /// [`target_column`](JoinKey::target_column) to that value through
    /// [`Vista::add_condition`]. The target's `query()` therefore records the
    /// join, and the target must take the constrained column. A driver that
    /// compares ids by type converts the value in its own `add_condition`.
    ///
    /// A contained relation, or a join whose public `keys` were emptied, has
    /// nothing to narrow by and is an `IncorrectUsage` error. Other errors
    /// carry `relation` and `column` context. A refusal (`Unsupported` or
    /// `Unimplemented`, such as a computed join column) stays a refusal; any
    /// other failure is a normal error.
    pub fn narrow(&self, mut target: Vista, row: &Record<CborValue>) -> Result<Vista> {
        if self.kind.is_contained() || self.keys.is_empty() {
            return Err(error!(
                "relation has no join keys to narrow by",
                relation = self.name.as_str(),
                kind = self.kind.as_str()
            )
            .mark_incorrect_usage());
        }
        for key in &self.keys {
            self.narrow_key(&mut target, key, row)?;
        }
        Ok(target)
    }

    fn narrow_key(&self, target: &mut Vista, key: &JoinKey, row: &Record<CborValue>) -> Result<()> {
        let relation = self.name.as_str();
        let value = row.get(&key.from).ok_or_else(|| {
            error!(
                "parent row lacks the join column",
                relation = relation,
                column = key.from.as_str()
            )
        })?;
        let column = match key.target_column(target) {
            Ok(column) => column.to_string(),
            Err(e) => return Err(wrap(e, relation, &key.from)),
        };
        target
            .add_condition(&column, FilterOp::Eq, value.clone())
            .map_err(|e| wrap(e, relation, &column))
    }
}

/// `err` as the source of a "can't narrow" error naming `relation` and
/// `column`, keeping a refusal's kind.
fn wrap(err: VantageError, relation: &str, column: &str) -> VantageError {
    let (unsupported, unimplemented) = (err.is_unsupported(), err.is_unimplemented());
    let wrapped = Err::<(), _>(err)
        .with_context(|| {
            error!(
                "can't narrow the relation's target to the parent row",
                relation = relation,
                column = column
            )
        })
        .unwrap_err();
    if unsupported {
        wrapped.mark_unsupported()
    } else if unimplemented {
        wrapped.mark_unimplemented()
    } else {
        wrapped
    }
}
