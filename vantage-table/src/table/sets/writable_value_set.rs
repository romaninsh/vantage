use std::future::Future;

use async_trait::async_trait;
use indexmap::IndexMap;
use tokio::time::sleep;
use vantage_core::{Backoff, Priority, Result};
use vantage_dataset::{Inserted, WritableValueSet};
use vantage_types::{Entity, InvariantValue, Record};
use vantage_vista::{Capability, forbidden};

use crate::{
    prelude::TableSource,
    table::HookReturn,
    table::Table,
    table::sets::{
        hooks::{UpdateOf, run_after, run_after_delete_all, run_before_delete, run_delete_all},
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
    ) -> Result<Inserted<Self::Value>> {
        if self.id_is_expression() {
            return Err(computed_id(self.table_name(), "insert"));
        }
        let record = self.prepare_insert(record).await?;
        let id = id.into();
        let (source, id_ref, record_ref) = (self.data_source(), &id, &record);
        // After an `Unavailable` attempt that may have landed, a row found
        // existing is this call's own write: the hooks below run once for it.
        let result = self
            .retry_unavailable(move |after_unavailable| async move {
                match source.insert_table_value(self, id_ref, record_ref).await {
                    Ok(Inserted::Existed(stored)) if after_unavailable => {
                        Ok(Inserted::Created(stored))
                    }
                    out => out,
                }
            })
            .await?;
        // An `Existed` insert wrote nothing, so no after-insert hook runs;
        // `Accepted` has no stored row, so the hooks see the row as sent.
        match &result {
            Inserted::Created(stored) => self.finish_insert(&id, stored).await?,
            Inserted::Accepted => self.finish_insert(&id, &record).await?,
            Inserted::Existed(_) => {}
        }
        Ok(result)
    }

    async fn replace_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.require_allowed(Capability::Update)?;
        self.require_confined_writes("replace")?;
        if self.id_is_expression() {
            return Err(computed_id(self.table_name(), "replace"));
        }
        let id = id.into();
        let needs_former = self.kept_columns().next().is_some()
            || self.denials().rule_for(Capability::Insert).is_some()
            || self.merges_contained(record);
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
        // A dotted key under a contained relation merges into the stored
        // record, so the row is read for it.
        let former = self
            .former_for_update(&id, self.merges_contained(&partial))
            .await?;
        // The row was read for the hooks and the set doesn't hold it: the
        // source would refuse the patch anyway, so the hooks don't see it.
        let read = !self.before_update_hooks().is_empty() || !self.after_update_hooks().is_empty();
        if read && former.is_none() {
            return Err(
                vantage_core::error!("row isn't in this set", table = self.table_name())
                    .mark_not_found(),
            );
        }
        let update = UpdateOf {
            id: &id,
            stored: former.as_ref(),
            patch: true,
        };
        // Strips an expression column a before-update hook set; it isn't
        // stored either way.
        self.before_write(self.before_update_hooks(), &mut partial, Some(update))
            .await?;
        validate_invariants(&partial, &self.typed_invariants())?;
        let result = self
            .retry_unavailable(|_| self.data_source().patch_table_value(self, &id, &partial))
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
        let delete = |_: bool| self.data_source().delete_table_value(self, &id);
        if !has_hooks {
            return self.retry_unavailable(delete).await;
        }
        // Read through the set: a row outside it (or a missing one) gets no
        // hooks and no delete — the source would leave it alone anyway, and
        // running hooks for it would act on a row this set doesn't hold.
        let Some(former) = self.stored_value(&id).await? else {
            return Ok(());
        };
        let erased = self.as_entity_erased();
        let outcome = run_before_delete(self.before_delete_hooks(), &id, &former, erased).await?;
        let deleted = match outcome {
            HookReturn::Proceed => self.delete_judged(self, &id, &former).await?,
            HookReturn::Handled => former,
        };
        run_after(self.after_delete_hooks(), &id, &deleted, None, erased).await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        self.require_allowed(Capability::Delete)?;
        self.require_allowed(Capability::DeleteAll)?;
        let erased = self.as_entity_erased();
        run_delete_all(self.before_delete_all_hooks(), erased).await?;
        if self.before_delete_hooks().is_empty() {
            self.retry_unavailable(|_| self.data_source().delete_table_all_values(self))
                .await?;
        } else {
            self.delete_row_by_row().await?;
        }
        run_after_delete_all(self.after_delete_all_hooks(), erased).await
    }
}

/// A write that names an id on a table whose id the database computes from
/// the row: the id would be ignored, so the write is refused.
fn computed_id(table: &str, write: &str) -> vantage_core::VantageError {
    vantage_core::error!(
        format!(
            "`{table}` computes its id from the row, so {write} can't name one; \
             insert without an id, or patch the row"
        ),
        table = table
    )
    .mark_unsupported()
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue,
{
    /// Insert `record` into a table whose id is an expression and return the
    /// id the database computed with the outcome. A row in the set matching
    /// the record is `Existed`, so the insert is retry-safe: it retries an
    /// unavailable source as [`WritableValueSet::insert_value`] does, and
    /// runs the after-insert hooks only for a row this call created.
    pub async fn insert_derived_value(
        &self,
        record: &Record<T::Value>,
    ) -> Result<(T::Id, Inserted<T::Value>)> {
        let record = self.prepare_insert(record).await?;
        let (source, record_ref) = (self.data_source(), &record);
        let (id, result) = self
            .retry_unavailable(move |after_unavailable| async move {
                match source.insert_table_derived_id_value(self, record_ref).await {
                    Ok((id, Inserted::Existed(stored))) if after_unavailable => {
                        Ok((id, Inserted::Created(stored)))
                    }
                    out => out,
                }
            })
            .await?;
        match &result {
            Inserted::Created(stored) => self.finish_insert(&id, stored).await?,
            Inserted::Accepted => self.finish_insert(&id, &record).await?,
            Inserted::Existed(_) => {}
        }
        Ok((id, result))
    }

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
            None => Ok(self
                .insert_value(id, record)
                .await?
                .into_record()
                .unwrap_or_else(|| record.clone())),
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
            let deleted = if proceed {
                self.delete_judged(&whole, id, former).await?
            } else {
                former.clone()
            };
            run_after(self.after_delete_hooks(), id, &deleted, None, erased).await?;
        }
        Ok(())
    }

    /// Delete `id` from `table` once its before-delete hooks passed on
    /// `former`, retrying an unavailable source. A retry may follow an
    /// attempt that landed, and the id may hold another row by then, so it
    /// re-reads the row: gone means the earlier attempt deleted it, and a row
    /// still there goes through the before-delete hooks again before it is
    /// deleted. Returns the row the after-delete hooks get.
    async fn delete_judged<E2: Entity<T::Value>>(
        &self,
        table: &Table<T, E2>,
        id: &T::Id,
        former: &Record<T::Value>,
    ) -> Result<Record<T::Value>> {
        self.retry_unavailable(|retried| async move {
            if !retried {
                self.data_source().delete_table_value(table, id).await?;
                return Ok(former.clone());
            }
            let Some(current) = table.stored_value(id).await? else {
                return Ok(former.clone());
            };
            let erased = self.as_entity_erased();
            if let HookReturn::Proceed =
                run_before_delete(self.before_delete_hooks(), id, &current, erased).await?
            {
                self.data_source().delete_table_value(table, id).await?;
            }
            Ok(current)
        })
        .await
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
        // A key spelled as a stored name or dotted under a contained host
        // answers to each such column's rule (see `columns_of_key`).
        record.retain(|name, _| {
            self.columns_of_key(name)
                .into_iter()
                .all(|column| self.denials().column_rule(column).is_none())
        });
        // A read-only contained host goes back after the contained check: its
        // stored value is the store's own, which may hold fields from before
        // the contained table, so it isn't checked again.
        let mut kept_hosts = Vec::new();
        if let Some(former) = &former {
            for name in self.kept_columns() {
                if let Some(value) = former.get(name) {
                    if self.contained.iter().any(|rel| rel.host_column() == name) {
                        kept_hosts.push((name.to_string(), value.clone()));
                    } else {
                        record.insert(name.to_string(), value.clone());
                    }
                }
            }
        }
        let update = UpdateOf {
            id: &id,
            stored: former.as_ref(),
            patch: false,
        };
        self.before_write(self.before_update_hooks(), &mut record, Some(update))
            .await?;
        for (name, stored) in kept_hosts {
            let value = match record.get(&name) {
                Some(written) => self.restore_ruled_fields(&name, written, &stored)?,
                None => stored,
            };
            let value = self.data_source().coerce_column_value(self, &name, value)?;
            record.insert(name, value);
        }
        enforce_invariants(&mut record, &self.typed_invariants())?;
        let result = self
            .retry_unavailable(|_| self.data_source().replace_table_value(self, &id, &record))
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

    /// A ruled contained host the replace wrote (`written`, after the
    /// contained merge): the fields under it that a rule covers take their
    /// stored values back, and the rest of the write stands. A contains_many
    /// host has no fields to pick from, so it keeps its stored value whole.
    fn restore_ruled_fields(
        &self,
        host: &str,
        written: &T::Value,
        stored: &T::Value,
    ) -> Result<T::Value> {
        let many = self
            .contained
            .iter()
            .find(|rel| rel.host_column() == host)
            .is_some_and(|rel| rel.kind() == vantage_vista::ReferenceKind::ContainsMany);
        let source = self.data_source();
        let (Some(mut written), false) = (source.contained_records(written)?, many) else {
            return Ok(stored.clone());
        };
        let stored = source
            .contained_records(stored)?
            .and_then(|records| records.into_iter().next())
            .unwrap_or_default();
        written.truncate(1);
        if written.is_empty() {
            written.push(Record::new());
        }
        let record = &mut written[0];
        let prefix = format!("{host}.");
        let ruled = self
            .denials()
            .columns()
            .filter_map(|(name, _)| {
                let path = self.nested_column_path(name).unwrap_or(name);
                let field = path.strip_prefix(&prefix)?;
                // A rule deeper down (`a.b.c`) keeps the whole first-level field.
                Some(field.split('.').next().unwrap_or(field).to_string())
            })
            .collect::<Vec<_>>();
        for field in ruled {
            match stored.get(&field) {
                Some(value) => record.insert(field, value.clone()),
                None => record.shift_remove(&field),
            };
        }
        source.contained_host(written, false)
    }

    /// The stored columns a rule made read-only: a replace keeps their
    /// stored values. So is a contained host with a rule on something under
    /// it (a nested column or its stored path).
    fn kept_columns(&self) -> impl Iterator<Item = &str> {
        let ruled_hosts = self
            .contained
            .iter()
            .map(|rel| rel.host_column())
            .filter(|host| {
                self.denials().column_rule(host).is_none()
                    && self
                        .columns_of_key(host)
                        .into_iter()
                        .any(|column| self.denials().column_rule(column).is_some())
            });
        self.denials()
            .columns()
            .map(|(name, _)| name)
            .filter(|name| !self.is_calculated_column(name))
            .chain(ruled_hosts)
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
    /// Hooks see this as `stored`, and kept columns write it back. Each
    /// contained host is read even when the projection leaves it out: a
    /// dotted write merges into the host as stored.
    async fn stored_value(&self, id: &T::Id) -> Result<Option<Record<T::Value>>> {
        self.check_read().await?;
        let hosts = self.contained.iter().map(|rel| rel.host_column());
        if hosts.clone().all(|host| self.is_active_column(host)) {
            return self.data_source().get_table_value(self, id).await;
        }
        let mut whole = self.clone();
        if let Some(active) = &mut whole.active_columns {
            active.extend(hosts.map(str::to_string));
        }
        self.data_source().get_table_value(&whole, id).await
    }

    /// Every row of the set as stored; see [`Self::stored_value`].
    async fn stored_values(&self) -> Result<IndexMap<T::Id, Record<T::Value>>> {
        self.check_read().await?;
        self.data_source().list_table_values(self).await
    }

    /// Runs a source write once, or, for an `Essential` request outside a
    /// transaction, again after each `Unavailable` failure, with the essential
    /// back-off between attempts and no attempt limit: the caller's deadline
    /// drops the future. `op` is told whether an earlier attempt failed
    /// `Unavailable` (and so may have landed). Inside a transaction a failed
    /// statement has already failed the whole transaction, so only the
    /// transaction can be retried.
    async fn retry_unavailable<R, F, Fut>(&self, mut op: F) -> Result<R>
    where
        F: FnMut(bool) -> Fut,
        Fut: Future<Output = Result<R>>,
    {
        let retries =
            Priority::current().is_essential() && self.data_source().transaction().is_none();
        let mut attempt = 0;
        loop {
            match op(attempt > 0).await {
                Err(e) if retries && e.is_unavailable() => {
                    sleep(Backoff::ESSENTIAL.delay(attempt)).await;
                    attempt += 1;
                }
                out => return out,
            }
        }
    }
}
