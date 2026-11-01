use async_trait::async_trait;
use vantage_core::Result;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_types::{Entity, InvariantValue, Record};

use crate::{
    prelude::TableSource,
    table::HookReturn,
    table::Table,
    table::sets::{
        hooks::{
            UpdateOf, run_after, run_after_delete_all, run_before, run_before_delete,
            run_delete_all,
        },
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
        let former = self.former_for_update_hooks(&id).await?;
        let update = UpdateOf {
            id: &id,
            stored: former.as_ref(),
            patch: false,
        };
        run_before(
            self.before_update_hooks(),
            &mut record,
            Some(update),
            erased,
        )
        .await?;
        self.strip_imported_columns(&mut record);
        self.strip_expression_columns(&mut record);
        enforce_invariants(&mut record, self.invariants())?;
        let result = self
            .data_source()
            .replace_table_value(self, &id, &record)
            .await?;
        run_after(
            self.after_update_hooks(),
            &id,
            &result,
            former.as_ref(),
            erased,
        )
        .await?;
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
        self.refuse_expression_columns(partial)?;
        let erased = self.as_entity_erased();
        let mut partial = partial.clone();
        let former = self.former_for_update_hooks(&id).await?;
        let update = UpdateOf {
            id: &id,
            stored: former.as_ref(),
            patch: true,
        };
        run_before(
            self.before_update_hooks(),
            &mut partial,
            Some(update),
            erased,
        )
        .await?;
        self.strip_imported_columns(&mut partial);
        validate_invariants(&partial, self.invariants())?;
        let result = self
            .data_source()
            .patch_table_value(self, &id, &partial)
            .await?;
        run_after(
            self.after_update_hooks(),
            &id,
            &result,
            former.as_ref(),
            erased,
        )
        .await?;
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
        run_after(self.after_delete_hooks(), &id, &former, None, erased).await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let erased = self.as_entity_erased();
        run_delete_all(self.before_delete_all_hooks(), erased).await?;
        self.data_source().delete_table_all_values(self).await?;
        run_after_delete_all(self.after_delete_all_hooks(), erased).await
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue,
{
    /// The row as it stands before an update, read once through the set, when
    /// any before- or after-update hook will want it; `None` when there are
    /// none (no extra read) or the set doesn't hold the row.
    async fn former_for_update_hooks(&self, id: &T::Id) -> Result<Option<Record<T::Value>>> {
        if self.before_update_hooks().is_empty() && self.after_update_hooks().is_empty() {
            return Ok(None);
        }
        self.get_value(id.clone()).await
    }
}
