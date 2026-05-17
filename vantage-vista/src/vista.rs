use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::{
    capabilities::VistaCapabilities,
    column::Column,
    flags,
    reference::{Reference, ReferenceKind},
    sort::SortDirection,
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
/// from a YAML schema. The schema (columns, references, id column) lives
/// on the wrapped [`TableShell`] — `Vista` is the user-facing surface that
/// forwards both data and metadata queries to the shell.
///
/// Cross-persistence references (`with_foreign`) are the one exception:
/// they're registered at the Vista layer because they need to capture
/// other-backend factories outside the shell's scope.
pub struct Vista {
    pub(crate) name: String,
    pub(crate) foreign_resolvers: IndexMap<String, ForeignRef>,
    pub(crate) capabilities: VistaCapabilities,
    pub source: Box<dyn TableShell>,
}

impl Vista {
    pub fn new(name: impl Into<String>, source: Box<dyn TableShell>) -> Self {
        let capabilities = source.capabilities().clone();
        Self {
            name: name.into(),
            foreign_resolvers: IndexMap::new(),
            capabilities,
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

    // ---- metadata accessors -----------------------------------------------
    //
    // All schema accessors forward to the shell. Vista holds none of the
    // schema state itself; the shell is the source of truth.

    pub fn get_id_column(&self) -> Option<&str> {
        self.source.id_column()
    }

    /// Columns flagged `title` (in declaration order).
    pub fn get_title_columns(&self) -> Vec<&str> {
        self.source
            .columns()
            .values()
            .filter(|c| c.is_title())
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn get_column_names(&self) -> Vec<&str> {
        self.source.columns().keys().map(String::as_str).collect()
    }

    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.source.columns().get(name)
    }

    /// Names of references attached at the Vista layer — cross-persistence
    /// resolvers from [`with_foreign`](Self::with_foreign) and shell-declared
    /// references. For the *complete* picture with cardinality, use
    /// [`list_references`](Self::list_references) instead.
    pub fn get_references(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.foreign_resolvers.keys().map(String::as_str).collect();
        for k in self.source.references().keys() {
            let s = k.as_str();
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// All references the Vista exposes, with their cardinality.
    ///
    /// Combines two sources: cross-persistence resolvers attached via
    /// [`with_foreign`](Self::with_foreign), and same-persistence
    /// references declared by the wrapped shell. Each is returned once,
    /// foreign first, with later duplicates ignored.
    pub fn list_references(&self) -> Vec<(String, ReferenceKind)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (name, fref) in &self.foreign_resolvers {
            if seen.insert(name.clone()) {
                out.push((name.clone(), fref.kind));
            }
        }
        for (name, r) in self.source.references() {
            if seen.insert(name.clone()) {
                out.push((name.clone(), r.kind));
            }
        }
        out
    }

    pub fn get_reference(&self, name: &str) -> Option<&Reference> {
        self.source.references().get(name)
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

    // ---- pagination -------------------------------------------------------

    /// Declare how many records constitute one page. Some backends (notably
    /// REST APIs with server-fixed page sizes) refuse this — check
    /// `capabilities().can_set_page_size` before calling.
    pub fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.source.set_page_size(size)
    }

    /// Fetch a specific page (1-based) using offset-style pagination. The
    /// per-page count comes from the most recent
    /// [`set_page_size`](Self::set_page_size). Returns `Unsupported` when the
    /// driver does not advertise `can_fetch_page`; cursor-only drivers
    /// (DynamoDB, most token-paginated REST APIs) only support
    /// [`fetch_next`](Self::fetch_next) instead.
    pub async fn fetch_page(&self, page: usize) -> Result<Vec<(String, Record<CborValue>)>> {
        self.source.fetch_page(self, page).await
    }

    /// Cursor-style chain fetch. Pass `None` on the first call; pass the
    /// previous call's returned token on subsequent calls. Returned token is
    /// `None` when the result set is exhausted.
    ///
    /// The token is **driver-private** — its shape is whatever the backend
    /// uses (DynamoDB `LastEvaluatedKey`, REST `nextToken`, offset count,
    /// …). Consumers treat it as opaque and round-trip it back unchanged.
    /// Returns `Unsupported` when the driver does not advertise
    /// `can_fetch_next`.
    pub async fn fetch_next(
        &self,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        self.source.fetch_next(self, token).await
    }

    // ---- quicksearch -------------------------------------------------------

    /// Apply a quicksearch filter. The driver decides which columns participate;
    /// typically those carrying the [`SEARCHABLE`](crate::flags::SEARCHABLE)
    /// flag.
    ///
    /// **Replace semantics**: calling `add_search` again drops the previous
    /// search filter. Returns `Unsupported` when the driver does not advertise
    /// `can_search`.
    pub fn add_search(&mut self, text: impl Into<String>) -> Result<()> {
        self.source.add_search(&text.into())
    }

    /// Drop any quicksearch filter previously applied. Returns `Unsupported`
    /// from the driver shell when search is unsupported.
    pub fn clear_search(&mut self) -> Result<()> {
        self.source.clear_search()
    }

    // ---- ordering ---------------------------------------------------------

    /// Sort results by `column` in the given direction.
    ///
    /// **Replace semantics**: calling `add_order` again wipes the previous
    /// order and pushes the new one. V1 supports a single sort column only;
    /// multi-column sort can be added later without renaming.
    ///
    /// Returns `Unsupported` when the column is not flagged
    /// [`ORDERABLE`](crate::flags::ORDERABLE) — drivers like DynamoDB only
    /// flag their declared sort-key columns. Returns `Unsupported` from the
    /// driver shell when the driver itself does not support ordering at all
    /// (`capabilities().can_order == false`).
    pub fn add_order(&mut self, column: &str, dir: SortDirection) -> Result<()> {
        let col = self
            .source
            .columns()
            .get(column)
            .ok_or_else(|| error!("Unknown column for add_order", column = column))?;
        if !col.has_flag(flags::ORDERABLE) {
            return Err(error!(
                format!("column '{}' is not orderable", column),
                column = column
            )
            .is_unsupported());
        }
        self.source.add_order(column, dir)
    }

    /// Wipe every sort previously applied through `add_order`. Returns
    /// `Unsupported` from the driver shell when ordering is unsupported.
    pub fn clear_orders(&mut self) -> Result<()> {
        self.source.clear_orders()
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
    ///    [`TableShell::get_ref`], which consults the wrapped typed `Table`'s
    ///    `with_one` / `with_many` registrations.
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
