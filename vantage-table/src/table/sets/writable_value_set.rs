use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::WritableValueSet;
use vantage_types::{Entity, InvariantValue, Record};
use vantage_vista::{Capability, forbidden};

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
        self.require_allowed(Capability::Update)?;
        self.require_confined_writes("replace")?;
        let id = id.into();
        let needs_former = self.kept_columns().next().is_some()
            || self.denials().rule_for(Capability::Insert).is_some();
        let former = self.former_for_update(&id, needs_former).await?;
        self.replace_over(id, record, former).await
    }

    async fn patch_value(
        &self,
        id: impl Into<Self::Id> + Send,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.require_allowed(Capability::Update)?;
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
        self.refuse_read_only_columns(partial)?;
        let erased = self.as_entity_erased();
        let mut partial = partial.clone();
        let former = self.former_for_update(&id, false).await?;
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
        // A before-update hook may have set one; it isn't stored either way.
        self.strip_expression_columns(&mut partial);
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
        self.require_allowed(Capability::Delete)?;
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
        let Some(former) = self.stored_value(&id).await? else {
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
        self.require_allowed(Capability::Delete)?;
        self.require_allowed(Capability::DeleteAll)?;
        let erased = self.as_entity_erased();
        run_delete_all(self.before_delete_all_hooks(), erased).await?;
        if self.before_delete_hooks().is_empty() {
            self.data_source().delete_table_all_values(self).await?;
        } else {
            self.delete_row_by_row().await?;
        }
        run_after_delete_all(self.after_delete_all_hooks(), erased).await
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue,
{
    /// Write `record` under `id` as an insert when the set doesn't hold the
    /// row and as a replace when it does, so the insert hooks (the id
    /// generator, a created-at stamp) run only on a new row and the update
    /// hooks see the stored one. A table with no insert or update hooks
    /// skips the read and replaces, which creates a missing row.
    ///
    /// With hooks it reads the row through the set, so it needs read access,
    /// and a row that exists outside the table's conditions takes the insert
    /// path.
    pub async fn upsert_value(
        &self,
        id: impl Into<T::Id> + Send,
        record: &Record<T::Value>,
    ) -> Result<Record<T::Value>> {
        let id = id.into();
        let hooked = !self.before_insert_hooks().is_empty()
            || !self.after_insert_hooks().is_empty()
            || !self.before_update_hooks().is_empty()
            || !self.after_update_hooks().is_empty();
        if !hooked {
            return self.replace_value(id, record).await;
        }
        self.require_allowed(Capability::Update)?;
        self.require_confined_writes("upsert")?;
        match self.stored_value(&id).await? {
            None => self.insert_value(id, record).await,
            Some(former) => self.replace_over(id, record, Some(former)).await,
        }
    }

    /// `delete_all` on a table with before-delete hooks: every row of the
    /// set is judged first, so a refusal deletes nothing, then each row is
    /// deleted on its own and its after-delete hooks run. The set is the
    /// conditioned one, whole: a pagination window shapes reads only, as it
    /// does for the one-statement delete.
    async fn delete_row_by_row(&self) -> Result<()> {
        self.require_confined_writes("delete")?;
        let erased = self.as_entity_erased();
        let mut whole = erased.clone();
        whole.set_pagination(None);
        let rows = whole.stored_values().await?;
        let mut proceed = Vec::with_capacity(rows.len());
        for (id, former) in &rows {
            let outcome = run_before_delete(self.before_delete_hooks(), id, former, erased).await?;
            proceed.push(matches!(outcome, HookReturn::Proceed));
        }
        for ((id, former), proceed) in rows.iter().zip(proceed) {
            if proceed {
                self.data_source().delete_table_value(&whole, id).await?;
            }
            run_after(self.after_delete_hooks(), id, former, None, erased).await?;
        }
        Ok(())
    }

    /// The replace after its checks, over the row as the set holds it
    /// (`former`, `None` when it doesn't or nothing asked for it).
    async fn replace_over(
        &self,
        id: T::Id,
        record: &Record<T::Value>,
        former: Option<Record<T::Value>>,
    ) -> Result<Record<T::Value>> {
        let erased = self.as_entity_erased();
        // A replace of a row that isn't there creates it.
        if let (Some(rule), None) = (self.denials().rule_for(Capability::Insert), &former) {
            return forbidden(Capability::Insert, rule, self.table_name());
        }
        let mut record = record.clone();
        // A whole row read and saved back carries the read-only columns too:
        // each keeps its stored value, so an unchanged round-trip saves and a
        // backend that rewrites the whole row doesn't erase it.
        record.retain(|name, _| self.denials().column_rule(name).is_none());
        if let Some(former) = &former {
            for name in self.kept_columns() {
                if let Some(value) = former.get(name) {
                    record.insert(name.to_string(), value.clone());
                }
            }
        }
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

    /// The stored columns a rule made read-only: a replace keeps their
    /// stored values.
    fn kept_columns(&self) -> impl Iterator<Item = &str> {
        self.denials()
            .columns()
            .map(|(name, _)| name)
            .filter(|name| !self.is_calculated_column(name))
    }

    /// The row as it stands before an update, read once through the set, when
    /// the caller `needs` it or any before- or after-update hook will want it;
    /// `None` when neither does (no extra read) or the set doesn't hold the
    /// row.
    async fn former_for_update(&self, id: &T::Id, needs: bool) -> Result<Option<Record<T::Value>>> {
        if !needs && self.before_update_hooks().is_empty() && self.after_update_hooks().is_empty() {
            return Ok(None);
        }
        self.stored_value(id).await
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// The row as the set stores it, for the write path: read through the
    /// set's conditions and read check, but without the `AfterLoad` hooks
    /// and lazy expressions, which rewrite rows for callers reading them.
    /// Hooks see this as `stored`, and kept columns write it back.
    async fn stored_value(&self, id: &T::Id) -> Result<Option<Record<T::Value>>> {
        self.check_read().await?;
        self.data_source().get_table_value(self, id).await
    }

    /// Every row of the set as stored; see [`Self::stored_value`].
    async fn stored_values(&self) -> Result<IndexMap<T::Id, Record<T::Value>>> {
        self.check_read().await?;
        self.data_source().list_table_values(self).await
    }
}
