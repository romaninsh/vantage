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

/// A before-write hook: mutate the record in place ahead of invariant
/// enforcement, returning `Err` to cancel the write. Receives the record being
/// written and the (entity-erased) table for relation/datasource access.
pub type BeforeFn<T> = Arc<
    dyn for<'r> Fn(
            &'r mut Record<<T as TableSource>::Value>,
            &'r Table<T, EmptyEntity>,
        ) -> Pin<Box<dyn Future<Output = vantage_core::Result<()>> + Send + 'r>>
        + Send
        + Sync,
>;

/// A before-delete hook: receives the row id and its current contents; may
/// `Err` to veto, or return [`HookReturn::Handled`] to take over (soft-delete).
pub type BeforeDeleteFn<T> = Arc<
    dyn for<'r> Fn(
            &'r <T as TableSource>::Id,
            &'r Record<<T as TableSource>::Value>,
            &'r Table<T, EmptyEntity>,
        )
            -> Pin<Box<dyn Future<Output = vantage_core::Result<HookReturn>> + Send + 'r>>
        + Send
        + Sync,
>;

/// An after-commit hook: side-effects on the committed row (id + record). Used
/// for inserts, updates, and deletes (where the record is the former contents).
pub type AfterFn<T> = Arc<
    dyn for<'r> Fn(
            &'r <T as TableSource>::Id,
            &'r Record<<T as TableSource>::Value>,
            &'r Table<T, EmptyEntity>,
        ) -> Pin<Box<dyn Future<Output = vantage_core::Result<()>> + Send + 'r>>
        + Send
        + Sync,
>;

/// A lifecycle hook attached to a table via [`Table::with_hook`]. Each variant
/// carries a closure receiving exactly the references available at that stage.
/// `BeforeSave`/`AfterSave` are sugar that register for both insert and update.
pub enum Hook<T: TableSource> {
    BeforeInsert(Phase, BeforeFn<T>),
    BeforeUpdate(Phase, BeforeFn<T>),
    BeforeSave(Phase, BeforeFn<T>),
    BeforeDelete(BeforeDeleteFn<T>),
    AfterInsert(AfterFn<T>),
    AfterUpdate(AfterFn<T>),
    AfterSave(AfterFn<T>),
    AfterDelete(AfterFn<T>),
}

/// A table's registered lifecycle hooks, split by placement. The before-write
/// bands are kept ordered by [`Phase`]. Populated via [`Table::with_hook`].
#[derive(Clone)]
pub struct Hooks<T: TableSource> {
    pub(crate) before_insert: Vec<(Phase, BeforeFn<T>)>,
    pub(crate) before_update: Vec<(Phase, BeforeFn<T>)>,
    pub(crate) before_delete: Vec<BeforeDeleteFn<T>>,
    pub(crate) after_insert: Vec<AfterFn<T>>,
    pub(crate) after_update: Vec<AfterFn<T>>,
    pub(crate) after_delete: Vec<AfterFn<T>>,
}

impl<T: TableSource> Default for Hooks<T> {
    fn default() -> Self {
        Self {
            before_insert: Vec::new(),
            before_update: Vec::new(),
            before_delete: Vec::new(),
            after_insert: Vec::new(),
            after_update: Vec::new(),
            after_delete: Vec::new(),
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

    /// Register a lifecycle [`Hook`]. Before-write hooks are kept ordered by
    /// [`Phase`] (then registration order); `BeforeSave`/`AfterSave` register
    /// for both insert and update.
    pub fn add_hook(&mut self, hook: Hook<T>) {
        let hooks = &mut self.hooks;
        match hook {
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
        }
    }

    /// Builder form of [`Self::add_hook`].
    pub fn with_hook(mut self, hook: Hook<T>) -> Self {
        self.add_hook(hook);
        self
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
