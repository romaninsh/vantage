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

// A row written into a scoped set must conform however it is written, so
// insert, replace and patch all enforce the set's invariants.
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
        let record = self.prepare_insert(record).await?;
        let id = id.into();
        let result = self
            .data_source()
            .insert_table_value(self, &id, &record)
            .await?;
        self.finish_insert(&id, &result).await?;
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
