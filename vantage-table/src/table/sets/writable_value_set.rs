use async_trait::async_trait;
use vantage_core::Result;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_types::{Entity, InvariantValue, Record};

use crate::{
    prelude::TableSource,
    table::HookReturn,
    table::Table,
    table::sets::{
        hooks::{run_after, run_before, run_before_delete},
        invariants::{enforce_invariants, validate_invariants},
    },
};

// Implement WritableValueSet by running lifecycle hooks and enforcing set
// invariants, then delegating to the data source. A row written into a scoped
// set must conform however it is written, so insert/replace/patch all run
// invariants; before/after hooks fire around each operation.
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> WritableValueSet for Table<T, E>
where
    T::Value: InvariantValue,
{
    async fn insert_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.require_confined_writes("insert")?;
        let id = id.into();
        let erased = self.as_entity_erased();
        let mut record = record.clone();
        run_before(self.before_insert_hooks(), &mut record, erased).await?;
        self.strip_imported_columns(&mut record);
        enforce_invariants(&mut record, self.invariants())?;
        let result = self
            .data_source()
            .insert_table_value(self, &id, &record)
            .await?;
        run_after(self.after_insert_hooks(), &id, &result, erased).await?;
        Ok(result)
    }

    async fn replace_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.require_confined_writes("replace")?;
        let id = id.into();
        let erased = self.as_entity_erased();
        let mut record = record.clone();
        run_before(self.before_update_hooks(), &mut record, erased).await?;
        self.strip_imported_columns(&mut record);
        enforce_invariants(&mut record, self.invariants())?;
        let result = self
            .data_source()
            .replace_table_value(self, &id, &record)
            .await?;
        run_after(self.after_update_hooks(), &id, &result, erased).await?;
        Ok(result)
    }

    async fn patch_value(
        &self,
        id: impl Into<Self::Id> + Send,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.require_confined_writes("patch")?;
        let id = id.into();
        // A patch is explicit intent per key — unlike a full-record round-trip
        // there is nothing incidental to strip, so a read-only imported column
        // in the payload is an error, not a silent drop (stripping it could
        // even leave an empty patch that "succeeds" while changing nothing).
        if let Some(name) = partial.keys().find(|k| self.is_imported_column(k)) {
            return Err(vantage_core::error!(
                "cannot patch read-only implicit-reference column",
                column = name.as_str()
            ));
        }
        let erased = self.as_entity_erased();
        let mut partial = partial.clone();
        run_before(self.before_update_hooks(), &mut partial, erased).await?;
        self.strip_imported_columns(&mut partial);
        validate_invariants(&partial, self.invariants())?;
        let result = self
            .data_source()
            .patch_table_value(self, &id, &partial)
            .await?;
        run_after(self.after_update_hooks(), &id, &result, erased).await?;
        Ok(result)
    }

    async fn delete(&self, id: impl Into<Self::Id> + Send) -> Result<()> {
        self.require_confined_writes("delete")?;
        let id = id.into();
        let has_hooks =
            !self.before_delete_hooks().is_empty() || !self.after_delete_hooks().is_empty();
        if !has_hooks {
            return self.data_source().delete_table_value(self, &id).await;
        }
        // Read through the set: a row outside it (or a missing one) gets no
        // hooks and no delete — the source would leave it alone anyway, and
        // running hooks for it would act on a row this set doesn't hold.
        let Some(former) = self.get_value(id.clone()).await? else {
            return Ok(());
        };
        let erased = self.as_entity_erased();
        let outcome = run_before_delete(self.before_delete_hooks(), &id, &former, erased).await?;
        if let HookReturn::Proceed = outcome {
            self.data_source().delete_table_value(self, &id).await?;
        }
        run_after(self.after_delete_hooks(), &id, &former, erased).await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        self.data_source().delete_table_all_values(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_dataset::ReadableValueSet;
    use vantage_types::{EmptyEntity, Record};

    #[tokio::test]
    async fn test_writable_value_set_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
        ];

        let mock_source = MockTableSource::new()
            .with_data("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, EmptyEntity>::new("test_table", mock_source);

        // Test insert_value with new ID
        let new_record = Record::from(json!({"name": "Charlie", "age": 35}));
        let inserted = table.insert_value("3", &new_record).await.unwrap();
        assert_eq!(inserted["id"], json!("3"));
        assert_eq!(inserted["name"], json!("Charlie"));
        assert_eq!(inserted["age"], json!(35));

        // insert_value with an existing ID returns the stored row
        let duplicate_record = Record::from(json!({"name": "David", "age": 40}));
        assert_eq!(
            table.insert_value("1", &duplicate_record).await.unwrap()["name"],
            json!("Alice")
        );

        // Test replace_value with existing ID
        let updated_record = Record::from(json!({"name": "Bob Updated", "age": 26}));
        let replaced = table.replace_value("2", &updated_record).await.unwrap();
        assert_eq!(replaced["id"], json!("2"));
        assert_eq!(replaced["name"], json!("Bob Updated"));
        assert_eq!(replaced["age"], json!(26));

        // Test replace_value with new ID (should create)
        let new_record2 = Record::from(json!({"name": "Eve", "age": 28}));
        let replaced2 = table.replace_value("4", &new_record2).await.unwrap();
        assert_eq!(replaced2["id"], json!("4"));
        assert_eq!(replaced2["name"], json!("Eve"));

        // Test patch_value
        let patch = Record::from(json!({"age": 31}));
        let patched = table.patch_value("1", &patch).await.unwrap();
        assert_eq!(patched["name"], json!("Alice")); // Original name preserved
        assert_eq!(patched["age"], json!(31)); // Age updated

        // Test patch_value with non-existing ID should fail
        let patch2 = Record::from(json!({"age": 50}));
        let result2 = table.patch_value("999", &patch2).await;
        assert!(result2.is_err());

        // Test delete
        table.delete("2").await.unwrap();
        let result3 = table.get_value("2").await.unwrap();
        assert!(result3.is_none()); // Should be deleted

        // delete of a missing id succeeds (idempotent)
        table.delete("999").await.unwrap();

        // Test delete_all
        table.delete_all().await.unwrap();
        let all_values = table.list_values().await.unwrap();
        assert_eq!(all_values.len(), 0); // All records should be deleted
    }

    #[tokio::test]
    async fn conditioned_table_refuses_writes_the_source_cannot_confine() {
        let src = MockTableSource::new()
            .without_confined_writes()
            .with_data("t", vec![json!({"id": "1", "parent": "p1"})])
            .await;
        let all = Table::<MockTableSource, EmptyEntity>::new("t", src.clone());
        // A literal equality the mock could confine on its own, so only the
        // table-level guard can refuse it.
        let set = all
            .clone()
            .with_condition(src.eq_value_condition("parent", json!("p1")).unwrap());
        assert!(set.delete("1").await.unwrap_err().is_unsupported());
        assert!(
            set.insert_value("2", &Record::from(json!({})))
                .await
                .unwrap_err()
                .is_unsupported()
        );
        all.delete("1").await.unwrap(); // unconditioned: allowed
    }

    #[tokio::test]
    async fn delete_outside_the_set_skips_hooks_and_the_delete() {
        use crate::table::{AfterFn, Hook};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let src = MockTableSource::new()
            .with_data(
                "t",
                vec![
                    json!({"id": "in", "parent": "p1"}),
                    json!({"id": "out", "parent": "p2"}),
                ],
            )
            .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let after: AfterFn<MockTableSource> = Arc::new(move |_id, _row, _t| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        let all = Table::<MockTableSource, EmptyEntity>::new("t", src).with_id_column("id");
        let set = all
            .clone()
            .with_condition(vantage_expressions::Expression::new(
                "{} = {}",
                vec![
                    vantage_expressions::ExpressiveEnum::Nested(
                        vantage_expressions::Expression::new("parent", vec![]),
                    ),
                    vantage_expressions::ExpressiveEnum::Nested(
                        vantage_expressions::Expression::new(
                            "{}",
                            vec![vantage_expressions::ExpressiveEnum::Scalar(json!("p1"))],
                        ),
                    ),
                ],
            ))
            .with_hook(Hook::AfterDelete(after));

        set.delete("out").await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no hooks for a row outside the set"
        );
        assert!(
            all.get_value("out").await.unwrap().is_some(),
            "the row outside the set stays"
        );
        set.delete("in").await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        set.delete("in").await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a missing row runs no hooks"
        );
    }
}
