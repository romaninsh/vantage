//! Apply a table's set invariants to a record before it is written.
//!
//! A `Table` narrowed by a literal `column = value` (an id scope or a traversed
//! relation) carries that pair as an *invariant* (see `Table::invariants`).
//! Full-record writes conform (fill absent/null columns, reject a different
//! value); patches validate (never fill, reject a null or different value).
//! Both the typed-entity and the raw-record write paths funnel through here.

use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::{InvariantValue, Record};

/// Conform `record` to `invariants`: fill absent/null columns, keep equal
/// values, `Conflict` on a different one.
pub(crate) fn enforce_invariants<V: InvariantValue>(
    record: &mut Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    vantage_dataset::invariants::conform(record, invariants)
}

/// Check a patch against `invariants` without adding anything.
pub(crate) fn validate_invariants<V: InvariantValue>(
    partial: &Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    vantage_dataset::invariants::validate(partial, invariants)
}

#[cfg(test)]
mod tests {
    use crate::mocks::mock_table_source::MockTableSource;
    use crate::table::Table;
    use serde_json::json;
    use vantage_dataset::prelude::{InsertableValueSet, ReadableValueSet};
    use vantage_types::{EmptyEntity, Record};

    // Backend-agnostic: exercises the 4-way decision table through the generic
    // value-set path on a non-SQL source (serde_json values).
    #[tokio::test]
    async fn invariant_enforced_on_generic_backend() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = Table::<MockTableSource, EmptyEntity>::new("t", src)
            .with_invariant("parent_id", json!("p1"));

        // absent → filled
        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "absent"})))
            .await
            .unwrap();
        assert_eq!(
            table.get_value(id).await.unwrap().unwrap()["parent_id"],
            json!("p1")
        );

        // present null → filled
        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "null", "parent_id": null})))
            .await
            .unwrap();
        assert_eq!(
            table.get_value(id).await.unwrap().unwrap()["parent_id"],
            json!("p1")
        );

        // present and matching → kept (no error)
        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "match", "parent_id": "p1"})))
            .await
            .unwrap();
        assert_eq!(
            table.get_value(id).await.unwrap().unwrap()["parent_id"],
            json!("p1")
        );

        // present and conflicting → rejected
        let result = table
            .insert_return_id_value(&Record::from(
                json!({"name": "wrong", "parent_id": "other"}),
            ))
            .await;
        assert!(
            result.is_err(),
            "conflicting invariant value must be rejected"
        );
    }

    #[tokio::test]
    async fn patch_through_with_id_does_not_write_the_id() {
        use vantage_dataset::prelude::WritableValueSet;
        let src = MockTableSource::new()
            .with_data(
                "t",
                vec![json!({"id": "1", "name": "a", "parent_id": "p1"})],
            )
            .await;
        let base = Table::<MockTableSource, EmptyEntity>::new("t", src).with_id_column("id");
        let one = base.clone().with_id(json!("1")).unwrap();
        let patched = one
            .patch_value("1", &Record::from(json!({"name": "b"})))
            .await
            .unwrap();
        assert_eq!(patched["name"], json!("b"));
        let scoped = base.with_invariant("parent_id", json!("p1"));
        let p = scoped
            .patch_value("1", &Record::from(json!({"name": "c"})))
            .await
            .unwrap();
        assert_eq!(p["parent_id"], json!("p1"));
        let err = scoped
            .patch_value("1", &Record::from(json!({"parent_id": "p2"})))
            .await
            .unwrap_err();
        assert!(err.is_conflict(), "{err}");
    }

    #[tokio::test]
    async fn conflicting_insert_is_a_conflict() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = Table::<MockTableSource, EmptyEntity>::new("t", src)
            .with_invariant("parent_id", json!("p1"));
        let err = table
            .insert_return_id_value(&Record::from(json!({"parent_id": "other"})))
            .await
            .unwrap_err();
        assert!(err.is_conflict(), "{err}");
    }
}
