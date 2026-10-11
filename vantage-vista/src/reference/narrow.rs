//! [`Reference::narrow`]: narrowing a join's target to one parent row.

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, VantageError, error};
use vantage_types::Record;

use super::{JoinKey, Reference};
use crate::filter::FilterOp;
use crate::vista::Vista;

impl Reference {
    /// Narrow this join's bare `target` to the rows related to `row`.
    ///
    /// Each key reads the parent's `from` column, passes the value through
    /// `coerce` (a driver that compares ids by type re-tags string ids here;
    /// others pass it through unchanged), and constrains the target's
    /// [`target_column`](JoinKey::target_column) through
    /// [`Vista::add_condition`]. The target's `query()` therefore records the
    /// join, and the target must declare the constrained column.
    ///
    /// Errors carry `relation` and `column` context. A refusal (`Unsupported`
    /// or `Unimplemented`, such as a computed join column) stays a refusal;
    /// any other failure is a normal error.
    pub fn narrow(
        &self,
        mut target: Vista,
        row: &Record<CborValue>,
        coerce: impl Fn(&CborValue) -> CborValue,
    ) -> Result<Vista> {
        for key in &self.keys {
            self.narrow_key(&mut target, key, row, &coerce)?;
        }
        Ok(target)
    }

    fn narrow_key(
        &self,
        target: &mut Vista,
        key: &JoinKey,
        row: &Record<CborValue>,
        coerce: &impl Fn(&CborValue) -> CborValue,
    ) -> Result<()> {
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
            .add_condition(&column, FilterOp::Eq, coerce(value))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::MockShell;
    use crate::{Capability, Column, ColumnFlag, ReferenceKind, VistaMetadata};

    fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
            .collect()
    }

    fn target(metadata: VistaMetadata) -> Vista {
        Vista::new(
            "order",
            MockShell::new()
                .with_metadata(metadata)
                .with_capabilities([Capability::FilterOps].into_iter().collect()),
        )
    }

    fn orders() -> Reference {
        Reference::new(
            "orders",
            "order",
            ReferenceKind::HasMany,
            vec![JoinKey::new("id", "client_id")],
        )
    }

    #[test]
    fn narrows_through_the_vista_with_the_coerced_value() {
        let schema = VistaMetadata::new()
            .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
            .with_column(Column::new("client_id", "string"))
            .with_id_column("id");
        let narrowed = orders()
            .narrow(target(schema), &rec(&[("id", "alice")]), |v| match v {
                CborValue::Text(s) => CborValue::Text(format!("client:{s}")),
                other => other.clone(),
            })
            .unwrap();
        let condition = &narrowed.query().conditions[0];
        assert_eq!(condition.column, "client_id");
        assert_eq!(condition.value, CborValue::Text("client:alice".into()));
    }

    #[test]
    fn a_missing_parent_column_names_relation_and_column() {
        let err = orders()
            .narrow(target(VistaMetadata::new()), &rec(&[]), CborValue::clone)
            .unwrap_err();
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"orders\"")
        );
        assert_eq!(
            err.context.get("column").map(String::as_str),
            Some("\"id\"")
        );
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn a_computed_join_column_stays_a_refusal() {
        let schema = VistaMetadata::new()
            .with_column(Column::new("id", "string"))
            .with_column(
                Column::new("client_id", "string")
                    .with_expression("row.id")
                    .unwrap(),
            )
            .with_id_column("id");
        let err = orders()
            .narrow(target(schema), &rec(&[("id", "alice")]), CborValue::clone)
            .unwrap_err();
        assert!(err.is_unsupported(), "{err:?}");
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"orders\"")
        );
        assert_eq!(
            err.context.get("column").map(String::as_str),
            Some("\"client_id\"")
        );
    }

    #[test]
    fn a_target_without_an_id_column_names_the_relation() {
        let client = Reference::new(
            "client",
            "client",
            ReferenceKind::HasOne,
            vec![JoinKey::to_target_id("client_id")],
        );
        let err = client
            .narrow(
                target(VistaMetadata::new()),
                &rec(&[("client_id", "a")]),
                CborValue::clone,
            )
            .unwrap_err();
        assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"client\"")
        );
        assert_eq!(
            err.context.get("column").map(String::as_str),
            Some("\"client_id\"")
        );
    }
}
