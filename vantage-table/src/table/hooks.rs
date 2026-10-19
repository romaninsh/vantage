//! Lifecycle hooks attached to a [`Table`].
//!
//! Register them with [`Table::with_hook`] / [`Table::add_hook`]. Before-write
//! hooks run ahead of set-invariant enforcement, ordered by [`Phase`]; the
//! firing itself lives in the `table::sets` write paths.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vantage_types::{EmptyEntity, Entity, Record};

use crate::table::Table;
use crate::traits::table_source::TableSource;
use crate::write_context::WriteContext;

/// Ordering band for before-write hooks. Hooks run in this order (and in
/// registration order within a band), ahead of set-invariant enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Clean inputs (trim, default empties).
    Normalize,
    /// Set or compute fields (audit stamps, derived values). The default.
    Populate,
    /// Read-only checks, last — so they see the final record.
    Validate,
}

/// What a `before_delete` hook decided.
pub enum HookReturn {
    /// Carry out the underlying operation.
    Proceed,
    /// The hook already performed the operation (e.g. a soft-delete patch); skip
    /// the real one and report success.
    Handled,
}

/// The future a hook returns.
pub type BoxFuture<'r, X> = Pin<Box<dyn Future<Output = X> + Send + 'r>>;

/// What a before-write hook receives: the record about to be written (mutable),
/// the write's [`WriteContext`] if one is in scope, and the entity-erased table.
///
/// On an update (replace or patch), `id` is the row being written and
/// `stored` the row as the set holds it now, read once before the hooks
/// run; `stored` is `None` when the set doesn't hold the row. Both are
/// `None` on insert.
///
/// `patch` is true on a patch, whose `record` holds only the patched
/// fields: a hook judging the whole row reads the rest from `stored`. On a
/// replace, `record` is the whole new row and fields it lacks are dropped.
#[non_exhaustive]
pub struct BeforeCx<'r, T: TableSource> {
    pub record: &'r mut Record<T::Value>,
    pub id: Option<&'r T::Id>,
    pub stored: Option<&'r Record<T::Value>>,
    pub patch: bool,
    pub context: Option<&'r WriteContext>,
    pub table: &'r Table<T, EmptyEntity>,
}

/// What an after-commit hook receives. `record` is the committed row (the former
/// contents for a delete); `before` is the row as it was before an update, and
/// `None` for inserts and deletes.
#[non_exhaustive]
pub struct AfterCx<'r, T: TableSource> {
    pub id: &'r T::Id,
    pub record: &'r Record<T::Value>,
    pub before: Option<&'r Record<T::Value>>,
    pub context: Option<&'r WriteContext>,
    pub table: &'r Table<T, EmptyEntity>,
}

/// What a before-delete hook receives: the row id and its current contents.
#[non_exhaustive]
pub struct DeleteCx<'r, T: TableSource> {
    pub id: &'r T::Id,
    pub record: &'r Record<T::Value>,
    pub context: Option<&'r WriteContext>,
    pub table: &'r Table<T, EmptyEntity>,
}

/// What a bulk-delete hook receives: the table whose whole set is being
/// deleted. Its `conditions()` describe the set.
#[non_exhaustive]
pub struct DeleteAllCx<'r, T: TableSource> {
    pub context: Option<&'r WriteContext>,
    pub table: &'r Table<T, EmptyEntity>,
}

/// A bulk-delete hook, before (an `Err` cancels) or after `delete_all`.
pub type DeleteAllFn<T> = Arc<
    dyn for<'r> Fn(DeleteAllCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>> + Send + Sync,
>;

/// Build a [`DeleteAllFn`] from a closure.
pub fn delete_all_hook<T, F>(f: F) -> DeleteAllFn<T>
where
    T: TableSource,
    F: for<'r> Fn(DeleteAllCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}

/// A before-write hook: mutate the record in place ahead of invariant
/// enforcement, returning `Err` to cancel the write.
pub type BeforeFn<T> =
    Arc<dyn for<'r> Fn(BeforeCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>> + Send + Sync>;

/// A before-delete hook: may `Err` to veto, or return [`HookReturn::Handled`]
/// to take over (soft-delete).
pub type BeforeDeleteFn<T> = Arc<
    dyn for<'r> Fn(DeleteCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<HookReturn>>
        + Send
        + Sync,
>;

/// An after-commit hook: side-effects on the committed row. Used for inserts,
/// updates, and deletes.
pub type AfterFn<T> =
    Arc<dyn for<'r> Fn(AfterCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>> + Send + Sync>;

/// Build a [`BeforeFn`] from a closure; the bound guides closure inference.
pub fn before_hook<T, F>(f: F) -> BeforeFn<T>
where
    T: TableSource,
    F: for<'r> Fn(BeforeCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}

/// Build a [`BeforeDeleteFn`] from a closure.
pub fn before_delete_hook<T, F>(f: F) -> BeforeDeleteFn<T>
where
    T: TableSource,
    F: for<'r> Fn(DeleteCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<HookReturn>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}

/// Build a [`BeforeFn`] from a synchronous closure over the record and the
/// write's context; an `Err` cancels the write. For callers with nothing to
/// await, such as a script evaluated in place.
pub fn before_hook_sync<T: TableSource>(
    f: impl Fn(&mut Record<T::Value>, Option<&WriteContext>) -> vantage_core::Result<()>
    + Send
    + Sync
    + 'static,
) -> BeforeFn<T> {
    Arc::new(move |cx: BeforeCx<'_, T>| {
        let outcome = f(cx.record, cx.context);
        Box::pin(std::future::ready(outcome))
    })
}

/// Build a [`BeforeFn`] from a synchronous closure over the whole
/// [`BeforeCx`]: the record, and on an update the id and the stored row.
pub fn before_hook_sync_cx<T: TableSource>(
    f: impl for<'r> Fn(BeforeCx<'r, T>) -> vantage_core::Result<()> + Send + Sync + 'static,
) -> BeforeFn<T> {
    Arc::new(move |cx: BeforeCx<'_, T>| Box::pin(std::future::ready(f(cx))))
}

/// Build a [`BeforeDeleteFn`] from a synchronous closure over the row id and
/// the write's context: `Ok` proceeds with the delete, an `Err` vetoes it.
pub fn before_delete_hook_sync<T: TableSource>(
    f: impl Fn(&T::Id, Option<&WriteContext>) -> vantage_core::Result<()> + Send + Sync + 'static,
) -> BeforeDeleteFn<T> {
    Arc::new(move |cx: DeleteCx<'_, T>| {
        let outcome = f(cx.id, cx.context).map(|()| HookReturn::Proceed);
        Box::pin(std::future::ready(outcome))
    })
}

/// Build an [`AfterFn`] from a closure.
pub fn after_hook<T, F>(f: F) -> AfterFn<T>
where
    T: TableSource,
    F: for<'r> Fn(AfterCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}

/// What a read hook receives: the table about to be read (its conditions
/// describe the set) and the [`WriteContext`] in scope, if any — the same
/// task-local a write sees, so one scope covers a request's reads and writes.
#[non_exhaustive]
pub struct SelectCx<'r, T: TableSource> {
    pub context: Option<&'r WriteContext>,
    pub table: &'r Table<T, EmptyEntity>,
}

/// A read hook: an `Err` refuses the read.
pub type SelectFn<T> =
    Arc<dyn for<'r> Fn(SelectCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>> + Send + Sync>;

/// Build a [`SelectFn`] from a closure.
pub fn select_hook<T, F>(f: F) -> SelectFn<T>
where
    T: TableSource,
    F: for<'r> Fn(SelectCx<'r, T>) -> BoxFuture<'r, vantage_core::Result<()>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(f)
}

/// A lifecycle hook attached to a table via [`Table::with_hook`]. Each variant
/// carries a closure receiving exactly the references available at that stage.
/// `BeforeSave`/`AfterSave` are sugar that register for both insert and update.
#[non_exhaustive]
pub enum Hook<T: TableSource> {
    /// Before every read that returns rows or facts about rows: list, get,
    /// get-some, streams (before the first item), count and the aggregates
    /// (sum/max/min). An `Err` refuses the read. Vista shells that read the
    /// source directly call [`Table::check_read`] first.
    ///
    /// It does **not** run when the table is embedded in another query —
    /// `related_in_condition`, `column_table_values_expr`, `get_*_query` and
    /// `get_expr_*` build expressions or subqueries, not reads of this table.
    /// Rules that must hold inside a subquery belong in the table's
    /// conditions, added when it is built.
    ///
    /// Reads a write makes run it too, so a refusing `BeforeSelect` also
    /// refuses: an update when before- or after-update hooks exist (the
    /// stored row is read first), a delete when delete hooks exist (the row is read for
    /// them), and a write whose backend reads the row back. A Dio serves
    /// reads from its cache, filled by its own loader, so reads through a
    /// Dio don't run the master table's `BeforeSelect`.
    BeforeSelect(SelectFn<T>),
    BeforeInsert(Phase, BeforeFn<T>),
    BeforeUpdate(Phase, BeforeFn<T>),
    BeforeSave(Phase, BeforeFn<T>),
    BeforeDelete(BeforeDeleteFn<T>),
    AfterInsert(AfterFn<T>),
    AfterUpdate(AfterFn<T>),
    AfterSave(AfterFn<T>),
    AfterDelete(AfterFn<T>),
    /// Before `delete_all`, once for the whole set; per-row delete hooks don't run.
    BeforeDeleteAll(DeleteAllFn<T>),
    AfterDeleteAll(DeleteAllFn<T>),
}

/// A table's registered lifecycle hooks, split by placement. The before-write
/// bands are kept ordered by [`Phase`]. Populated via [`Table::with_hook`].
#[derive(Clone)]
pub struct Hooks<T: TableSource> {
    pub(crate) before_select: Vec<SelectFn<T>>,
    pub(crate) before_insert: Vec<(Phase, BeforeFn<T>)>,
    pub(crate) before_update: Vec<(Phase, BeforeFn<T>)>,
    pub(crate) before_delete: Vec<BeforeDeleteFn<T>>,
    pub(crate) after_insert: Vec<AfterFn<T>>,
    pub(crate) after_update: Vec<AfterFn<T>>,
    pub(crate) after_delete: Vec<AfterFn<T>>,
    pub(crate) before_delete_all: Vec<DeleteAllFn<T>>,
    pub(crate) after_delete_all: Vec<DeleteAllFn<T>>,
}

impl<T: TableSource> Default for Hooks<T> {
    fn default() -> Self {
        Self {
            before_select: Vec::new(),
            before_insert: Vec::new(),
            before_update: Vec::new(),
            before_delete: Vec::new(),
            after_insert: Vec::new(),
            after_update: Vec::new(),
            after_delete: Vec::new(),
            before_delete_all: Vec::new(),
            after_delete_all: Vec::new(),
        }
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    pub(crate) fn before_insert_hooks(&self) -> &[(Phase, BeforeFn<T>)] {
        &self.hooks.before_insert
    }
    pub(crate) fn before_update_hooks(&self) -> &[(Phase, BeforeFn<T>)] {
        &self.hooks.before_update
    }
    pub(crate) fn before_delete_hooks(&self) -> &[BeforeDeleteFn<T>] {
        &self.hooks.before_delete
    }
    pub(crate) fn after_insert_hooks(&self) -> &[AfterFn<T>] {
        &self.hooks.after_insert
    }
    pub(crate) fn after_update_hooks(&self) -> &[AfterFn<T>] {
        &self.hooks.after_update
    }
    pub(crate) fn after_delete_hooks(&self) -> &[AfterFn<T>] {
        &self.hooks.after_delete
    }
    pub(crate) fn before_delete_all_hooks(&self) -> &[DeleteAllFn<T>] {
        &self.hooks.before_delete_all
    }
    pub(crate) fn after_delete_all_hooks(&self) -> &[DeleteAllFn<T>] {
        &self.hooks.after_delete_all
    }

    /// Register a lifecycle [`Hook`]. Before-write hooks are kept ordered by
    /// [`Phase`] (then registration order); `BeforeSave`/`AfterSave` register
    /// for both insert and update.
    pub fn add_hook(&mut self, hook: Hook<T>) {
        let hooks = &mut self.hooks;
        match hook {
            Hook::BeforeSelect(f) => hooks.before_select.push(f),
            Hook::BeforeInsert(phase, f) => push_phased(&mut hooks.before_insert, phase, f),
            Hook::BeforeUpdate(phase, f) => push_phased(&mut hooks.before_update, phase, f),
            Hook::BeforeSave(phase, f) => {
                push_phased(&mut hooks.before_insert, phase, f.clone());
                push_phased(&mut hooks.before_update, phase, f);
            }
            Hook::BeforeDelete(f) => hooks.before_delete.push(f),
            Hook::AfterInsert(f) => hooks.after_insert.push(f),
            Hook::AfterUpdate(f) => hooks.after_update.push(f),
            Hook::AfterSave(f) => {
                hooks.after_insert.push(f.clone());
                hooks.after_update.push(f);
            }
            Hook::AfterDelete(f) => hooks.after_delete.push(f),
            Hook::BeforeDeleteAll(f) => hooks.before_delete_all.push(f),
            Hook::AfterDeleteAll(f) => hooks.after_delete_all.push(f),
        }
    }

    /// Builder form of [`Self::add_hook`].
    pub fn with_hook(mut self, hook: Hook<T>) -> Self {
        self.add_hook(hook);
        self
    }

    /// Run the [`Hook::BeforeSelect`] hooks; an `Err` means the read is
    /// refused. Every `Table` read path calls this. Code that reads through
    /// the table's data source directly (a vista shell's count or window)
    /// must call it first.
    pub async fn check_read(&self) -> vantage_core::Result<()> {
        if self.hooks.before_select.is_empty() {
            return Ok(());
        }
        let context = WriteContext::current();
        let table = self.as_entity_erased();
        for hook in &self.hooks.before_select {
            hook(SelectCx {
                context: context.as_ref(),
                table,
            })
            .await?;
        }
        Ok(())
    }
}

/// Append a before-write hook and keep the vector ordered by phase (stable, so
/// registration order is preserved within a phase).
fn push_phased<T: TableSource>(
    hooks: &mut Vec<(Phase, BeforeFn<T>)>,
    phase: Phase,
    f: BeforeFn<T>,
) {
    hooks.push((phase, f));
    hooks.sort_by_key(|(p, _)| *p);
}
