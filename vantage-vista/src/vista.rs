use indexmap::IndexMap;
use vantage_core::{Result, error};

use crate::{
    capabilities::{Capability, VistaCapabilities, unsupported},
    column::{Column, ColumnFlag},
    page::Preview,
    query::VistaQuery,
    reference::Reference,
    shell::TableShell,
};

mod narrow;
mod read;
#[cfg(test)]
mod tests;
mod value_set;

/// Universal, schema-bearing data handle: the type-erased `Table<T, E>`.
///
/// A `Vista` is a value. It owns its query state, so narrowing a clone never
/// changes the original: `vista.clone().with_condition(..)` gives a new set.
/// The schema (columns, references, id column) lives on the wrapped
/// [`TableShell`]; every clause applied through the `Vista` is recorded in
/// [`query`](Self::query).
///
/// A `Vista` is single-persistence: every reference its shell exposes
/// resolves within the same backend. Joining across backends is the job of
/// the layer that holds the catalog.
#[derive(Clone)]
pub struct Vista {
    name: String,
    shell: Box<dyn TableShell>,
    applied: VistaQuery,
}

impl Vista {
    pub fn new(name: impl Into<String>, shell: impl TableShell) -> Self {
        Self::from_box(name.into(), Box::new(shell))
    }

    pub(crate) fn from_box(name: String, shell: Box<dyn TableShell>) -> Self {
        Self {
            name,
            shell,
            applied: VistaQuery::default(),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Wrap the shell, keeping the name and the applied query. Wrappers built
    /// with [`forward_table_shell!`](crate::forward_table_shell) take the
    /// inner shell this way.
    pub fn map_shell<S: TableShell>(self, wrap: impl FnOnce(Box<dyn TableShell>) -> S) -> Self {
        Self {
            shell: Box::new(wrap(self.shell)),
            ..self
        }
    }

    pub(crate) fn shell(&self) -> &dyn TableShell {
        &*self.shell
    }

    pub(crate) fn shell_mut(&mut self) -> &mut dyn TableShell {
        &mut *self.shell
    }

    // ---- introspection -----------------------------------------------------

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every clause applied through this Vista's `with_*` and `add_*`
    /// methods. A refused clause is not recorded.
    pub fn query(&self) -> &VistaQuery {
        &self.applied
    }

    /// Whether a condition, search or native condition limits this set.
    /// Order alone doesn't: it changes no membership.
    pub fn is_narrowed(&self) -> bool {
        self.applied.narrows()
    }

    /// The shell's capabilities, read live.
    pub fn capabilities(&self) -> &VistaCapabilities {
        self.shell.capabilities()
    }

    pub fn driver_name(&self) -> &'static str {
        self.shell.driver_name()
    }

    /// The query the next read would send, without sending it; see
    /// [`TableShell::preview`].
    pub fn preview(&self) -> Preview {
        self.shell.preview()
    }

    // ---- metadata ----------------------------------------------------------

    pub fn columns(&self) -> &IndexMap<String, Column> {
        self.shell.columns()
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.shell.columns().get(name)
    }

    pub fn id_column(&self) -> Option<&str> {
        self.shell.id_column()
    }

    /// Columns flagged [`Title`](ColumnFlag::Title), in declaration order.
    pub fn title_columns(&self) -> Vec<&str> {
        self.columns()
            .values()
            .filter(|c| c.has_flag(ColumnFlag::Title))
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn references(&self) -> &IndexMap<String, Reference> {
        self.shell.references()
    }

    pub fn reference(&self, name: &str) -> Option<&Reference> {
        self.shell.references().get(name)
    }

    /// A write is refused unless the shell advertises `capability`, whatever
    /// the shell itself would do: a read-only vista over a writable table
    /// takes no writes. Writes through a narrowed Vista also need a shell
    /// that keeps them inside the set.
    pub(crate) fn require_write(&self, capability: Capability, verb: &'static str) -> Result<()> {
        if !self.capabilities().has(capability) {
            return unsupported(capability);
        }
        if !self.is_narrowed() || self.capabilities().has(Capability::ConfineWrites) {
            return Ok(());
        }
        Err(error!(
            "this backend can't keep writes inside a narrowed set",
            verb = verb,
            table = self.name.as_str(),
            capability = Capability::ConfineWrites.as_str()
        )
        .mark_unsupported())
    }
}

impl std::fmt::Debug for Vista {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vista")
            .field("name", &self.name)
            .field("driver", &self.driver_name())
            .field("query", &self.applied)
            .finish()
    }
}
