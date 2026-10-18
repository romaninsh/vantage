//! Fire a table's lifecycle hooks around a write.
//!
//! Before-write hooks run on the record about to be written, in phase order,
//! just ahead of set-invariant enforcement; an error cancels the operation.
//! `before_delete` hooks may instead return [`HookReturn::Handled`] to take over
//! (soft-delete). After-commit hooks run for their side-effects. Hooks receive
//! the entity-erased table for relation/datasource access.

use vantage_core::Result;
use vantage_types::{EmptyEntity, Entity, InvariantValue, Record};

use super::invariants::enforce_invariants;
use crate::table::{
    AfterCx, AfterFn, BeforeCx, BeforeDeleteFn, BeforeFn, DeleteAllCx, DeleteAllFn, DeleteCx,
    HookReturn, Phase, Table,
};
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
    /// the confined-writes check, before-insert hooks, dropping read-only
    /// imported columns, and conforming to the set's invariants. Returns the
    /// record to store.
    pub async fn prepare_insert(&self, record: &Record<T::Value>) -> Result<Record<T::Value>> {
        self.require_confined_writes("insert")?;
        let mut record = record.clone();
        run_before(
            self.before_insert_hooks(),
            &mut record,
            self.as_entity_erased(),
        )
        .await?;
        self.strip_imported_columns(&mut record);
        enforce_invariants(&mut record, self.invariants())?;
        Ok(record)
    }

    /// Run the after-insert hooks for a record stored under `id`, as a single
    /// insert does once the driver write has succeeded.
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

pub(crate) async fn run_before<T: TableSource>(
    hooks: &[(Phase, BeforeFn<T>)],
    record: &mut Record<T::Value>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let context = WriteContext::current();
    for (_phase, hook) in hooks {
        hook(BeforeCx {
            record: &mut *record,
            context: context.as_ref(),
            table,
        })
        .await?;
    }
    Ok(())
}

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
