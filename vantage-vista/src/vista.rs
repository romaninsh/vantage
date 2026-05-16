use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::{
    capabilities::VistaCapabilities,
    column::Column,
    metadata::VistaMetadata,
    reference::{Reference, ReferenceKind},
    source::TableShell,
};

/// Closure that resolves a cross-persistence reference from a known parent row.
///
/// The closure receives a `Record<CborValue>` (the parent record carrying the
/// join value) and returns a fully-constructed `Vista` from any backend. The
/// closure captures whichever target factory it needs at definition time;
/// `Vista::with_foreign` stores it without ever invoking it.
pub type ForeignResolver = dyn Fn(&Record<CborValue>) -> Result<Vista> + Send + Sync;

/// One cross-persistence reference attached to a `Vista`.
pub struct ForeignRef {
    pub kind: ReferenceKind,
    pub resolver: Arc<ForeignResolver>,
}

impl std::fmt::Debug for ForeignRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForeignRef")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Universal, schema-bearing data handle.
///
/// A `Vista` is produced by a driver factory from a typed `Table<T, E>` or
/// from a YAML schema. Once built, its metadata is read-only — callers
/// observe via accessors and may narrow the data set with `add_condition_eq`,
/// which delegates to the underlying driver's native condition system. Vista
/// itself stores no condition state.
/// CRUD goes through the `ValueSet` trait family `Table<T, E>` also implements.
pub struct Vista {
    pub(crate) name: String,
    pub(crate) columns: IndexMap<String, Column>,
    pub(crate) references: IndexMap<String, Reference>,
    pub(crate) foreign_resolvers: IndexMap<String, ForeignRef>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) id_column: Option<String>,
    pub(crate) source: Box<dyn TableShell>,
}

impl Vista {
    pub fn new(
        name: impl Into<String>,
        source: Box<dyn TableShell>,
        metadata: VistaMetadata,
    ) -> Self {
        let capabilities = source.capabilities().clone();
        Self {
            name: name.into(),
            columns: metadata.columns,
            references: metadata.references,
            foreign_resolvers: IndexMap::new(),
            capabilities,
            id_column: metadata.id_column,
            source,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Override the vista's display name. Used by spec-driven construction
    /// to expose `spec.name` rather than the underlying file/table name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    /// Short human label for the underlying driver (e.g. `"csv"`, `"sqlite"`,
    /// `"postgres"`, `"mongodb"`).
    pub fn driver(&self) -> &'static str {
        self.source.driver_name()
    }

    pub(crate) fn source(&self) -> &dyn TableShell {
        self.source.as_ref()
    }

    // ---- metadata accessors -----------------------------------------------

    pub fn get_id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    /// Columns flagged `title` (in declaration order).
    pub fn get_title_columns(&self) -> Vec<&str> {
        self.columns
            .values()
            .filter(|c| c.is_title())
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn get_column_names(&self) -> Vec<&str> {
        self.columns.keys().map(String::as_str).collect()
    }

    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.columns.get(name)
    }

    /// Names of references attached at the Vista layer — cross-persistence
    /// resolvers from [`with_foreign`](Self::with_foreign) and YAML-loaded
    /// metadata refs. For the *complete* picture including the wrapped
    /// typed `Table`'s same-persistence refs (and their cardinality), use
    /// [`list_references`](Self::list_references) instead.
    pub fn get_references(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.foreign_resolvers.keys().map(String::as_str).collect();
        for k in self.references.keys() {
            let s = k.as_str();
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// All references the Vista exposes, with their cardinality.
    ///
    /// Combines three sources: cross-persistence resolvers attached via
    /// [`with_foreign`](Self::with_foreign), YAML-declared `references`
    /// metadata, and same-persistence refs surfaced by the driver shell
    /// (forwarded from the wrapped typed `Table`'s `with_one` / `with_many`
    /// registrations). Each is returned once, in the order
    /// foreign → metadata → shell, with later duplicates ignored.
    pub fn list_references(&self) -> Vec<(String, ReferenceKind)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (name, fref) in &self.foreign_resolvers {
            if seen.insert(name.clone()) {
                out.push((name.clone(), fref.kind));
            }
        }
        for (name, r) in &self.references {
            if seen.insert(name.clone()) {
                out.push((name.clone(), r.kind));
            }
        }
        for (name, kind) in self.source.get_ref_kinds() {
            if seen.insert(name.clone()) {
                out.push((name, kind));
            }
        }
        out
    }

    pub fn get_reference(&self, name: &str) -> Option<&Reference> {
        self.references.get(name)
    }

    // ---- conditions --------------------------------------------------------

    /// Narrow the vista to records matching `field == value`. Delegates to the
    /// underlying driver, which translates the value into its native condition
    /// type (BSON document for Mongo, `Expression` for CSV/SQL, …) and applies
    /// it to the wrapped table.
    ///
    /// Returns `Err` if the field is unknown to the driver or the value cannot
    /// be translated into the driver's condition vocabulary.
    pub fn add_condition_eq(&mut self, field: impl Into<String>, value: CborValue) -> Result<()> {
        self.source.add_eq_condition(&field.into(), &value)
    }

    /// Narrow to a single row by id.
    ///
    /// Convenience for the "I only know an id" workflow: pair with
    /// [`ReadableValueSet::get_some_value`](vantage_dataset::traits::ReadableValueSet::get_some_value)
    /// to fetch the row, then traverse via [`get_ref`](Self::get_ref).
    pub fn with_id(&mut self, id: impl Into<CborValue>) -> Result<&mut Self> {
        let id_column = self
            .get_id_column()
            .ok_or_else(|| error!("vista has no id column"))?
            .to_string();
        self.add_condition_eq(id_column, id.into())?;
        Ok(self)
    }

    // ---- aggregates (not part of ValueSet) ---------------------------------

    pub async fn get_count(&self) -> Result<i64> {
        self.source.get_vista_count(self).await
    }

    /// Push a driver-native condition into the wrapped table. The
    /// boxed value must match the driver's `T::Condition`. Used by
    /// YAML-driven relation factories that need to inject deferred
    /// FK eq-conditions (i.e. conditions whose value is resolved at
    /// fetch time by reading a parent record).
    pub fn add_raw_condition<C: Send + Sync + 'static>(&mut self, condition: C) -> Result<()> {
        self.source.add_raw_condition(Box::new(condition))
    }

    // ---- references --------------------------------------------------------

    /// Register a cross-persistence reference resolver.
    ///
    /// The closure is **stored, never invoked** at registration time — it
    /// fires exactly once, lazily, when [`get_ref`](Self::get_ref) is called
    /// for the relation. This guarantees that mutual references between two
    /// Vistas (A → B and B → A) don't recurse at construction.
    ///
    /// The `kind` argument records cardinality so consumers
    /// ([`list_references`](Self::list_references)) can render the right
    /// control — record card for `HasOne`, list grid for `HasMany`.
    ///
    /// The closure receives the parent's row at fire time. Cross-persistence
    /// joins on non-PK fields (e.g. `country.id = client.country_id`) work
    /// because the closure reads whichever field(s) it needs from the row.
    pub fn with_foreign(
        &mut self,
        relation: impl Into<String>,
        kind: ReferenceKind,
        resolver: impl Fn(&Record<CborValue>) -> Result<Vista> + Send + Sync + 'static,
    ) -> &mut Self {
        self.foreign_resolvers.insert(
            relation.into(),
            ForeignRef {
                kind,
                resolver: Arc::new(resolver),
            },
        );
        self
    }

    /// Traverse a named reference using a known source row.
    ///
    /// Routes in this order:
    /// 1. Cross-persistence resolvers registered via
    ///    [`with_foreign`](Self::with_foreign).
    /// 2. Same-persistence refs forwarded through
    ///    [`TableShell::get_ref`](crate::source::TableShell::get_ref), which
    ///    consults the wrapped typed `Table`'s `with_one` / `with_many`
    ///    registrations.
    ///
    /// The `row` must come from this Vista (typically via
    /// [`get_value`](vantage_dataset::traits::ReadableValueSet::get_value)
    /// or [`get_some_value`](vantage_dataset::traits::ReadableValueSet::get_some_value)).
    /// The join value is read out of the record and pushed as a plain
    /// eq-condition on the target — no subqueries, no deferred fetch.
    pub fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        if let Some(fref) = self.foreign_resolvers.get(relation) {
            return (fref.resolver)(row);
        }
        self.source.get_ref(relation, row)
    }
}
