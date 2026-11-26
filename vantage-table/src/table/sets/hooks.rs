//! Fire a table's lifecycle hooks around a write.
//!
//! After the Normalize and Populate hooks, each contained record the write
//! touches goes through its contained table and back into its host column
//! (see `write_contained`).
//! Before-write hooks run on the record about to be written, in phase order,
//! just ahead of set-invariant enforcement, with the source typing the
//! record between the Populate and Validate bands; an error cancels the
//! operation.
//! `before_delete` hooks may instead return [`HookReturn::Handled`] to take over
//! (soft-delete). After-commit hooks run for their side-effects: at once, or,
//! when the table's datasource is in a transaction, after it commits. Hooks
//! receive the entity-erased table for relation/datasource access.

use std::future::Future;

use indexmap::IndexMap;

use vantage_core::Result;
use vantage_types::{EmptyEntity, Entity, InvariantValue, Record};
use vantage_vista::{Capability, Transaction};

use super::invariants::enforce_invariants;
use crate::table::{
    AfterCx, AfterFn, BeforeCx, BeforeDeleteFn, BeforeFn, DeleteAllCx, DeleteAllFn, DeleteCx,
    HookReturn, Phase, Table,
};
use crate::traits::column_like::ColumnLike;
use crate::traits::table_source::TableSource;
use crate::write_context::WriteContext;

/// The two halves of a single-record insert that surround the driver's write.
/// A driver's bulk path calls them around its batch so that it stays
/// equivalent to inserting the records one by one.
impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue,
{
    /// Everything a single insert does to `record` before the driver write:
    /// the denial and confined-writes checks, before-insert hooks, dropping
    /// read-only imported and expression columns, and conforming to the
    /// set's invariants. Returns the record to store.
    pub async fn prepare_insert(&self, record: &Record<T::Value>) -> Result<Record<T::Value>> {
        self.require_allowed(Capability::Insert)?;
        self.require_confined_writes("insert")?;
        let mut record = record.clone();
        self.before_write(self.before_insert_hooks(), &mut record, None)
            .await?;
        enforce_invariants(&mut record, &self.typed_invariants())?;
        Ok(record)
    }

    /// Run the after-insert hooks for a record stored under `id`, as a single
    /// insert does once the driver write has succeeded. Inside a transaction
    /// they are held until it commits.
    pub async fn finish_insert(&self, id: &T::Id, record: &Record<T::Value>) -> Result<()> {
        run_after(
            self.after_insert_hooks(),
            id,
            record,
            None,
            self.as_entity_erased(),
        )
        .await
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// What every insert, replace and patch does to `record` before the
    /// set's invariants: the Normalize and Populate hooks, each contained
    /// record the write touches (as the hooks left it) checked through its
    /// contained table, refusing a column the table doesn't declare, dropping
    /// imported and expression columns, typing the declared columns through
    /// the source, then the Validate hooks, which so see the record as stored.
    pub(crate) async fn before_write(
        &self,
        hooks: &[(Phase, BeforeFn<T>)],
        record: &mut Record<T::Value>,
        update: Option<UpdateOf<'_, T>>,
    ) -> Result<()> {
        self.write_steps(hooks, record, update, false).await
    }

    /// The steps of [`Self::before_write`]. `contained`: this is a contained
    /// table checking a record for its host, so an id it doesn't declare is
    /// no column.
    async fn write_steps(
        &self,
        hooks: &[(Phase, BeforeFn<T>)],
        record: &mut Record<T::Value>,
        update: Option<UpdateOf<'_, T>>,
        contained: bool,
    ) -> Result<()> {
        let (shaping, validating) =
            hooks.split_at(hooks.partition_point(|(phase, _)| *phase < Phase::Validate));
        let erased = self.as_entity_erased();
        run_before(shaping, record, update, erased).await?;
        // After the shaping hooks, so a host value a hook set is checked too.
        Box::pin(self.write_contained(record, update)).await?;
        self.refuse_undeclared_columns(record, contained)?;
        self.strip_imported_columns(record);
        self.strip_expression_columns(record);
        self.coerce_record(record)?;
        run_before(validating, record, update, erased).await
    }

    /// Whether `record` has a dotted key under a contained host
    /// (`address.city`): such a write merges into the stored record, so it
    /// needs the stored row. A whole host value replaces it and needs none.
    pub(crate) fn merges_contained(&self, record: &Record<T::Value>) -> bool {
        self.contained.iter().any(|rel| {
            let host = rel.host_column();
            record
                .keys()
                .any(|key| dotted_under(self.write_path(key), host))
        })
    }

    /// The stored name of a nested column: one whose stored name (its alias)
    /// is dotted under a contained host, as `label` stored at `name.en_US`.
    /// A write to it is a write to that key of the contained record.
    pub(crate) fn nested_column_path(&self, key: &str) -> Option<&str> {
        let path = self.columns().get(key)?.alias()?;
        self.contained
            .iter()
            .any(|rel| dotted_under(path, rel.host_column()))
            .then_some(path)
    }

    /// Where a write key lands: a nested column's stored name, else the key.
    fn write_path<'k>(&'k self, key: &'k str) -> &'k str {
        self.nested_column_path(key).unwrap_or(key)
    }

    /// Put each contained record the write touches through its contained
    /// table, then back into the host column as one value. The change alone is
    /// checked (the host's whole value, or the dotted keys as a partial
    /// record): its columns are the contained table's (strict), its hooks run,
    /// and nothing is read or written. A partial record is merged into the
    /// stored one (`update`'s row as it stands), so fields stored there
    /// before stay as they are; a dotted key two levels down (`a.b.c`)
    /// merges into the stored `a.b` the same way. The contained table's
    /// update hooks get the stored record, under the host row's id.
    async fn write_contained(
        &self,
        record: &mut Record<T::Value>,
        update: Option<UpdateOf<'_, T>>,
    ) -> Result<()> {
        let stored = update.and_then(|u| u.stored);
        for rel in &self.contained {
            let host = rel.host_column();
            let many = rel.kind() == vantage_vista::ReferenceKind::ContainsMany;
            let whole = record.shift_remove(host);
            // Each key, with its field in the contained record.
            let dotted_keys: Vec<(String, String)> = record
                .keys()
                .filter_map(|key| {
                    let path = self.write_path(key);
                    dotted_under(path, host)
                        .then(|| (key.clone(), path[host.len() + 1..].to_string()))
                })
                .collect();
            if whole.is_none() && dotted_keys.is_empty() {
                continue;
            }
            if whole.is_some() && !dotted_keys.is_empty() {
                return Err(vantage_core::error!(
                    "a write gives a contained record both whole and by dotted keys",
                    table = self.table_name(),
                    relation = rel.name()
                ));
            }
            if many && !dotted_keys.is_empty() {
                return Err(vantage_core::error!(
                    "a contains_many relation is written as its whole array, not by dotted keys",
                    table = self.table_name(),
                    relation = rel.name(),
                    column = dotted_keys[0].0.as_str()
                )
                .mark_forbidden());
            }
            let target = rel.build_target(self.data_source().clone());
            let records = match whole {
                Some(value) => {
                    let mut records = self
                        .data_source()
                        .contained_records(&value)?
                        .unwrap_or_default();
                    if !many && records.len() > 1 {
                        return Err(vantage_core::error!(
                            "a contains_one relation holds one record",
                            table = self.table_name(),
                            relation = rel.name()
                        ));
                    }
                    for change in records.iter_mut() {
                        Box::pin(target.check_contained(change, None)).await?;
                    }
                    records
                }
                None => {
                    let mut change: Record<T::Value> = Record::new();
                    for (key, field) in &dotted_keys {
                        if let Some(value) = record.shift_remove(key) {
                            change.insert(field.clone(), value);
                        }
                    }
                    let stored = match stored.and_then(|row| row.get(host)) {
                        Some(value) => self
                            .data_source()
                            .contained_records(value)?
                            .unwrap_or_default(),
                        None => Vec::new(),
                    };
                    let mut base = stored.into_iter().next();
                    let child = match (update, base.as_ref()) {
                        (Some(u), Some(base)) => Some(UpdateOf {
                            id: u.id,
                            stored: Some(base),
                            patch: true,
                        }),
                        _ => None,
                    };
                    Box::pin(target.check_contained(&mut change, child)).await?;
                    let merged = base.get_or_insert_with(Record::new);
                    for (key, value) in change {
                        merged.insert(key, value);
                    }
                    base.into_iter().collect()
                }
            };
            record.insert(
                host.to_string(),
                self.data_source().contained_host(records, many)?,
            );
        }
        Ok(())
    }

    /// One contained record, or the changed part of the stored one
    /// (`update`), through this contained table's write steps with no read or
    /// write: the insert hooks for a new record or the update hooks for a
    /// change, its own contained records, the strict column check and the
    /// column typing.
    async fn check_contained(
        &self,
        record: &mut Record<T::Value>,
        update: Option<UpdateOf<'_, T>>,
    ) -> Result<()> {
        let hooks = if update.is_some() {
            self.before_update_hooks()
        } else {
            self.before_insert_hooks()
        };
        self.write_steps(hooks, record, update, true).await
    }

    /// Every column a write key stands for, as rules name them: the key
    /// itself, each column whose stored name the key is (`city` for
    /// `address.city`), and each contained host it is a dotted key under
    /// (`address` for `address.city`). A key can be several at once, and the
    /// write reaches all of them, so a rule on any one covers it. A nested
    /// column (`label` stored at `name.en_US`) stands for its host too.
    pub(crate) fn columns_of_key<'k>(&'k self, key: &'k str) -> Vec<&'k str> {
        let path = self.write_path(key);
        let mut columns = vec![key];
        columns.extend(
            self.columns()
                .iter()
                .filter(|(name, c)| c.alias() == Some(path) && name.as_str() != key)
                .map(|(name, _)| name.as_str()),
        );
        columns.extend(
            self.contained
                .iter()
                .map(|rel| rel.host_column())
                .filter(|host| dotted_under(path, host)),
        );
        columns
    }

    /// A write names only declared columns and the id, so a physical column
    /// the entity leaves out (`is_admin`), or a case variant of a declared
    /// one (`Salary` for `salary`) that a case-insensitive database would
    /// write, is refused, and so is one a hook set. A table with no columns
    /// accepts only the id.
    ///
    /// A key may also be a column's stored name in full (its alias,
    /// `fullName` or `place.venue_id`), never only the top of one: `place`
    /// alone would write the whole sub-document, on SQL an undeclared column.
    /// A contained relation's host column is declared: its value was just
    /// built by [`Self::write_contained`]. A `contained` record takes an id
    /// only when its table declares one.
    fn refuse_undeclared_columns(&self, record: &Record<T::Value>, contained: bool) -> Result<()> {
        let id_field = self.id_field_name();
        let id_declared = !contained || self.id_field().is_some();
        let declared = |key: &str| {
            (key == id_field && id_declared)
                || self.columns().contains_key(key)
                // Computed and imported columns are declared; the steps
                // after this drop them from a row read and saved back.
                || self.is_expression_column(key)
                || self.is_imported_column(key)
                || self.contained.iter().any(|rel| rel.host_column() == key)
                || self.columns().values().any(|c| c.alias() == Some(key))
        };
        match record.keys().find(|k| !declared(k.as_str())) {
            Some(column) => Err(vantage_core::error!(
                "column is not declared on this table",
                table = self.table_name(),
                column = column.as_str()
            )
            .mark_forbidden()),
            None => Ok(()),
        }
    }

    /// Each declared column of `record` other than the id, as the source
    /// stores it ([`TableSource::coerce_column_value`]).
    fn coerce_record(&self, record: &mut Record<T::Value>) -> Result<()> {
        let id_field = self.id_field_name();
        for (name, value) in record.iter_mut() {
            if *name == id_field || !self.columns().contains_key(name) {
                continue;
            }
            *value = self
                .data_source()
                .coerce_column_value(self, name, value.clone())?;
        }
        Ok(())
    }

    /// The set's invariants as the source stores their columns, so they
    /// compare with and stamp typed record values. A condition value is
    /// often loose text (`price = "5"`); one that doesn't fit its column
    /// stays as given and the comparison decides.
    pub(crate) fn typed_invariants(&self) -> IndexMap<String, T::Value> {
        let id_field = self.id_field_name();
        self.invariants()
            .iter()
            .map(|(name, value)| {
                let typed = if *name == id_field || !self.columns().contains_key(name) {
                    value.clone()
                } else {
                    self.data_source()
                        .coerce_column_value(self, name, value.clone())
                        .unwrap_or_else(|_| value.clone())
                };
                (name.clone(), typed)
            })
            .collect()
    }
}

/// The row an update writes, the row as the set holds it, and whether the
/// update is a patch (its record holds only the patched fields).
pub(crate) struct UpdateOf<'a, T: TableSource> {
    pub id: &'a T::Id,
    pub stored: Option<&'a Record<T::Value>>,
    pub patch: bool,
}

impl<T: TableSource> Clone for UpdateOf<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: TableSource> Copy for UpdateOf<'_, T> {}

/// `update` is `None` on insert.
pub(crate) async fn run_before<T: TableSource>(
    hooks: &[(Phase, BeforeFn<T>)],
    record: &mut Record<T::Value>,
    update: Option<UpdateOf<'_, T>>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let context = WriteContext::current();
    for (_phase, hook) in hooks {
        hook(BeforeCx {
            record: &mut *record,
            id: update.as_ref().map(|u| u.id),
            stored: update.as_ref().and_then(|u| u.stored),
            patch: update.as_ref().is_some_and(|u| u.patch),
            context: context.as_ref(),
            table,
        })
        .await?;
    }
    Ok(())
}

/// Inside a transaction the hooks are held until it commits.
pub(crate) async fn run_after<T: TableSource>(
    hooks: &[AfterFn<T>],
    id: &T::Id,
    record: &Record<T::Value>,
    before: Option<&Record<T::Value>>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let Some(tx) = table.data_source().transaction() else {
        return run_after_now(hooks, id, record, before, table).await;
    };
    let hooks = hooks.to_vec();
    let (id, record, before) = (id.clone(), record.clone(), before.cloned());
    after_commit(&tx, table, move |table| async move {
        run_after_now(&hooks, &id, &record, before.as_ref(), &table).await
    });
    Ok(())
}

/// Inside a transaction the hooks are held until it commits.
pub(crate) async fn run_after_delete_all<T: TableSource>(
    hooks: &[DeleteAllFn<T>],
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let Some(tx) = table.data_source().transaction() else {
        return run_delete_all(hooks, table).await;
    };
    let hooks = hooks.to_vec();
    after_commit(&tx, table, move |table| async move {
        run_delete_all(&hooks, &table).await
    });
    Ok(())
}

/// Whether `key` is dotted under `host` (`address.city` under `address`).
fn dotted_under(key: &str, host: &str) -> bool {
    key.strip_prefix(host)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Hold `work` until `tx` commits; a rollback drops it. It runs on a copy of
/// `table` back on its pool, the transaction being gone by then, under the
/// writer's [`WriteContext`].
fn after_commit<T, F, Fut>(tx: &Transaction, table: &Table<T, EmptyEntity>, work: F)
where
    T: TableSource,
    F: FnOnce(Table<T, EmptyEntity>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let mut outside = table.with_data_source(table.data_source().without_transaction());
    // Out of the transaction a locked read would wait on other writers' row locks.
    outside.lock = false;
    let context = WriteContext::current();
    tx.defer(Box::new(move || {
        Box::pin(async move {
            match context {
                Some(context) => context.scope(work(outside)).await,
                None => work(outside).await,
            }
        })
    }));
}

async fn run_after_now<T: TableSource>(
    hooks: &[AfterFn<T>],
    id: &T::Id,
    record: &Record<T::Value>,
    before: Option<&Record<T::Value>>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    let context = WriteContext::current();
    for hook in hooks {
        hook(AfterCx {
            id,
            record,
            before,
            context: context.as_ref(),
            table,
        })
        .await?;
    }
    Ok(())
}

pub(crate) async fn run_delete_all<T: TableSource>(
    hooks: &[DeleteAllFn<T>],
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let context = WriteContext::current();
    for hook in hooks {
        hook(DeleteAllCx {
            context: context.as_ref(),
            table,
        })
        .await?;
    }
    Ok(())
}

/// Run before-delete hooks; the first `Handled` short-circuits and signals the
/// caller to skip the underlying delete.
pub(crate) async fn run_before_delete<T: TableSource>(
    hooks: &[BeforeDeleteFn<T>],
    id: &T::Id,
    record: &Record<T::Value>,
    table: &Table<T, EmptyEntity>,
) -> Result<HookReturn> {
    let context = WriteContext::current();
    for hook in hooks {
        let cx = DeleteCx {
            id,
            record,
            context: context.as_ref(),
            table,
        };
        if let HookReturn::Handled = hook(cx).await? {
            return Ok(HookReturn::Handled);
        }
    }
    Ok(HookReturn::Proceed)
}
