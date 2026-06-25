//! Enforce a table's set invariants on a record before it is written.
//!
//! A `Table` narrowed by a literal `column = value` (an id scope or a traversed
//! relation) carries that pair as an *invariant* (see `Table::invariants`).
//! Every row written into the set must conform. This is the single, backend-
//! agnostic place that conformance is applied — both the typed-entity and the
//! raw-record write paths funnel through here.

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::{InvariantValue, Record};

/// Conform `record` to `invariants`, per column:
///
/// - absent → set to the invariant value
/// - present but null → set to the invariant value
/// - present and equal → keep
/// - present and different → `Err` (the row does not belong to this set)
pub(crate) fn enforce_invariants<V: InvariantValue>(
    record: &mut Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    for (column, expected) in invariants {
        match record.get(column) {
            None => {
                record.insert(column.clone(), expected.clone());
            }
            Some(value) if value.is_null() => {
                record.insert(column.clone(), expected.clone());
            }
            Some(value) if value.value_eq(expected) => {}
            Some(_) => {
                return Err(error!(
                    "value conflicts with the set it is written into",
                    column = column.as_str()
                ));
            }
        }
    }
    Ok(())
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
}
