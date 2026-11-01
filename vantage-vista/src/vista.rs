use indexmap::IndexMap;
use vantage_core::{Result, error};

use crate::{
    capabilities::{Capability, VistaCapabilities, unsupported},
    column::{Column, ColumnFlag},
    denials::{Denials, forbidden},
    id_generator::{IdGenerator, IdGeneratorShell},
    metadata::VistaMetadata,
    page::Preview,
    query::VistaQuery,
    reference::Reference,
    shell::TableShell,
};

mod narrow;
mod read;
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
    /// Added through [`deny`](Self::deny), on top of the shell metadata's.
    denials: Denials,
    /// The shell's capabilities less the denied ones, recomputed whenever
    /// the shell or the denials change.
    capabilities: VistaCapabilities,
}

impl Vista {
    pub fn new(name: impl Into<String>, shell: impl TableShell) -> Self {
        Self::from_box(name.into(), Box::new(shell))
    }

    pub(crate) fn from_box(name: String, shell: Box<dyn TableShell>) -> Self {
        let mut vista = Self {
            name,
            shell,
            applied: VistaQuery::default(),
            denials: Denials::default(),
            capabilities: VistaCapabilities::default(),
        };
        vista.refresh_capabilities();
        vista
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Wrap the shell, keeping the name and the applied query. Wrappers built
    /// with [`forward_table_shell!`](crate::forward_table_shell) take the
    /// inner shell this way.
    pub fn map_shell<S: TableShell>(self, wrap: impl FnOnce(Box<dyn TableShell>) -> S) -> Self {
        let mut vista = Self {
            shell: Box::new(wrap(self.shell)),
            ..self
        };
        vista.refresh_capabilities();
        vista
    }

    pub(crate) fn shell(&self) -> &dyn TableShell {
        &*self.shell
    }

    /// Run `edit` on the shell in place, then re-read its capabilities.
    pub(crate) fn edit_shell<R>(&mut self, edit: impl FnOnce(&mut dyn TableShell) -> R) -> R {
        let out = edit(&mut *self.shell);
        self.refresh_capabilities();
        out
    }

    fn refresh_capabilities(&mut self) {
        let mut capabilities = *self.shell.capabilities();
        for denials in [&self.shell.metadata().denials, &self.denials] {
            for denied in denials.capabilities().iter() {
                capabilities = capabilities.without(denied);
            }
        }
        self.capabilities = capabilities;
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

    /// The shell's capabilities less those a rule denies.
    pub fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    /// What rules forbid here: the shell metadata's denials and those added
    /// through [`deny`](Self::deny).
    pub fn denials(&self) -> Denials {
        let mut denials = self.metadata().denials.clone();
        denials.merge(&self.denials);
        denials
    }

    /// This Vista with `denials` added. Clones and narrowed copies carry
    /// them, and nothing takes one back.
    pub fn deny(mut self, denials: Denials) -> Self {
        self.denials.merge(&denials);
        self.refresh_capabilities();
        self
    }

    /// The rule denying `capability`, if one does.
    pub(crate) fn denied(&self, capability: Capability) -> Option<&str> {
        self.metadata()
            .denials
            .rule_for(capability)
            .or_else(|| self.denials.rule_for(capability))
    }

    /// The rule that made `column` read-only, if one did.
    pub(crate) fn denied_column(&self, column: &str) -> Option<&str> {
        self.metadata()
            .denials
            .column_rule(column)
            .or_else(|| self.denials.column_rule(column))
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

    fn metadata(&self) -> &VistaMetadata {
        self.shell.metadata()
    }

    pub fn columns(&self) -> &IndexMap<String, Column> {
        &self.metadata().columns
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns().get(name)
    }

    pub fn id_column(&self) -> Option<&str> {
        self.metadata().id_column.as_deref()
    }

    /// How a writer mints the id of a new record in this table; `None`
    /// means a UUIDv7.
    pub fn id_generator(&self) -> Option<&IdGenerator> {
        self.metadata().id_generator.as_ref()
    }

    /// This Vista with `generator` in its metadata, for a builder whose
    /// metadata the driver's factory made. Narrowing and clones keep it.
    pub fn with_id_generator(self, generator: IdGenerator) -> Self {
        self.map_shell(|inner| IdGeneratorShell::new(inner, generator))
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
        &self.metadata().references
    }

    pub fn reference(&self, name: &str) -> Option<&Reference> {
        self.references().get(name)
    }

    /// `forbidden` when a rule denies `capability`, else `unsupported` unless
    /// the shell advertises it: how `Vista` gates a read or write before the
    /// shell is called.
    pub(crate) fn require(&self, capability: Capability) -> Result<()> {
        if let Some(rule) = self.denied(capability) {
            return forbidden(capability, rule, &self.name);
        }
        if self.capabilities().has(capability) {
            Ok(())
        } else {
            unsupported(capability)
        }
    }

    /// A write is refused unless the shell advertises `capability`, whatever
    /// the shell itself would do: a read-only vista over a writable table
    /// takes no writes. Writes through a narrowed Vista also need a shell
    /// that keeps them inside the set.
    pub(crate) fn require_write(&self, capability: Capability, verb: &'static str) -> Result<()> {
        self.require(capability)?;
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
