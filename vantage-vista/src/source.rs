use std::pin::Pin;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use crate::{
    capabilities::VistaCapabilities,
    column::Column,
    reference::{ContainedSpec, Reference},
    sort::SortDirection,
    vista::Vista,
};

/// Per-driver executor for a `Vista`.
///
/// Implementations live in driver crates (vantage-sqlite, vantage-mongodb,
/// vantage-aws, etc.). Each method receives `&Vista` so the driver can read
/// the current condition state, columns, and other metadata.
///
/// `Id = String` and `Value = ciborium::Value` at this boundary, so every
/// driver's native id (Mongo `ObjectId`, Surreal `Thing`, …) stringifies
/// here. Methods are named with the `_vista_` infix to mirror
/// `TableSource`'s `_table_` convention; `Vista`'s `ValueSet` impls
/// delegate by stripping the infix.
///
/// `id: &String` (rather than `&str`) is intentional: the upstream
/// `vantage_dataset::ValueSet` trait family fixes `Id = String` and uses
/// `&Self::Id` in its signatures, so impls receive `&String` and forward
/// it through unchanged.
#[async_trait]
#[allow(clippy::ptr_arg)]
pub trait TableShell: Send + Sync + 'static {
    // ---- Schema --------------------------------------------------------------
    //
    // The shell owns the schema. `Vista` is a thin wrapper that forwards its
    // metadata accessors here. No defaults — every impl must answer (an empty
    // schema is a deliberate choice the impl declares explicitly).

    fn columns(&self) -> &IndexMap<String, Column>;

    fn references(&self) -> &IndexMap<String, Reference>;

    fn id_column(&self) -> Option<&str>;

    // ---- ReadableValueSet delegates ----------------------------------------

    async fn list_vista_values(&self, vista: &Vista)
    -> Result<IndexMap<String, Record<CborValue>>>;

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>>;

    /// Fetch one record by id, with the caller's existing (cheap) record
    /// available to drivers that can use it (e.g. a cmd detail script reading
    /// list-pass columns). The default ignores `row` and delegates to
    /// [`get_vista_value`](Self::get_vista_value); only drivers that benefit
    /// override it.
    async fn get_vista_value_with_row(
        &self,
        vista: &Vista,
        id: &String,
        _row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>> {
        self.get_vista_value(vista, id).await
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>>;

    /// Default implementation wraps `list_vista_values`. Drivers with native
    /// streaming (cursor-based queries, paginated REST APIs) override.
    #[allow(clippy::type_complexity)]
    fn stream_vista_values<'a>(
        &'a self,
        vista: &'a Vista,
    ) -> Pin<Box<dyn Stream<Item = Result<(String, Record<CborValue>)>> + Send + 'a>>
    where
        Self: Sync,
    {
        Box::pin(async_stream::stream! {
            match self.list_vista_values(vista).await {
                Ok(map) => {
                    for item in map {
                        yield Ok(item);
                    }
                }
                Err(e) => yield Err(e),
            }
        })
    }

    // ---- WritableValueSet delegates ----------------------------------------
    //
    // Default impls return a typed VantageError via `default_error` — drivers
    // override only what they actually support. The matching `VistaCapabilities`
    // flag must be set to `true` for any method the driver implements; if the
    // flag is `true` but the trait method falls through to the default,
    // `default_error` produces an `Unimplemented`-kind error (placeholder
    // detected). If the flag is `false`, it produces `Unsupported`. Both are
    // emitted as tracing events at construction.

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("insert_vista_value", "can_insert"))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("replace_vista_value", "can_update"))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
        _partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("patch_vista_value", "can_update"))
    }

    async fn delete_vista_value(&self, _vista: &Vista, _id: &String) -> Result<()> {
        Err(self.default_error("delete_vista_value", "can_delete"))
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        Err(self.default_error("delete_vista_all_values", "can_delete"))
    }

    // ---- InsertableValueSet delegate ---------------------------------------

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        _record: &Record<CborValue>,
    ) -> Result<String> {
        Err(self.default_error("insert_vista_return_id_value", "can_insert"))
    }

    // ---- Aggregates --------------------------------------------------------

    /// Default impl falls back to `list_vista_values` — drivers with native
    /// count (`SELECT COUNT(*)`, etc.) override.
    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Ok(self.list_vista_values(vista).await?.len() as i64)
    }

    // ---- Conditions --------------------------------------------------------

    /// Translate `field == value` into the driver's native condition type and
    /// apply it to the wrapped table. The default impl returns `Unimplemented`
    /// — every driver is expected to override.
    ///
    /// `value` is the universal CBOR carrier; the driver picks the appropriate
    /// translation (e.g. `cbor_to_bson` for Mongo, `cbor → AnyCsvType` for CSV).
    fn add_eq_condition(&mut self, _field: &str, _value: &CborValue) -> Result<()> {
        Err(error!(
            format!(
                "add_eq_condition not implemented for '{}'",
                std::any::type_name::<Self>()
            ),
            method = "add_eq_condition",
            source_type = std::any::type_name::<Self>()
        )
        .mark_unimplemented()
        .traced())
    }

    /// Push a driver-native condition into the wrapped table. The
    /// caller boxes the condition as `dyn Any` and the driver
    /// downcasts to its own `T::Condition`. Used by YAML-driven
    /// relation traversal, where the factory constructs a
    /// `DeferredFn`-bearing condition outside the value-set surface
    /// (which only accepts scalar eq) and pushes it through this
    /// channel. Default is `Unimplemented`.
    fn add_raw_condition(
        &mut self,
        _condition: Box<dyn std::any::Any + Send + Sync>,
    ) -> Result<()> {
        Err(error!(
            format!(
                "add_raw_condition not implemented for '{}'",
                std::any::type_name::<Self>()
            ),
            method = "add_raw_condition",
            source_type = std::any::type_name::<Self>()
        )
        .mark_unimplemented()
        .traced())
    }

    // ---- Pagination --------------------------------------------------------

    /// Declare how many records constitute one page. Used by both
    /// [`fetch_page`](Self::fetch_page) and [`fetch_next`](Self::fetch_next).
    /// Default returns `default_error("set_page_size", "can_set_page_size")`.
    fn set_page_size(&mut self, _size: usize) -> Result<()> {
        Err(self.default_error("set_page_size", "can_set_page_size"))
    }

    /// Fetch a specific page (1-based) using offset-style pagination. The
    /// per-page count comes from the most recent
    /// [`set_page_size`](Self::set_page_size).
    ///
    /// Drivers without random-access pagination (DynamoDB, most token-paginated
    /// REST APIs) leave the default in place, which produces `Unsupported`.
    /// Callers should branch on `vista.capabilities().can_fetch_page` first.
    async fn fetch_page(
        &self,
        _vista: &Vista,
        _page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        Err(self.default_error("fetch_page", "can_fetch_page"))
    }

    /// Cursor-style chain fetch. Pass `None` on the first call; pass the
    /// previous call's returned token on subsequent calls. Returned token is
    /// `None` when the result set is exhausted.
    ///
    /// The token is **driver-private** — its shape is whatever the backend
    /// finds convenient (DynamoDB `LastEvaluatedKey` as a CBOR map, REST
    /// `nextToken` as `CborValue::Text`, offset-based as `CborValue::Integer`).
    /// Consumers treat it as opaque and round-trip it back unchanged.
    ///
    /// Default returns `default_error("fetch_next", "can_fetch_next")`.
    async fn fetch_next(
        &self,
        _vista: &Vista,
        _token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        Err(self.default_error("fetch_next", "can_fetch_next"))
    }

    // ---- Quicksearch -------------------------------------------------------

    /// Apply a quicksearch filter — a single string the driver fans out across
    /// the columns it considers searchable (typically those flagged
    /// [`SEARCHABLE`](crate::flags::SEARCHABLE), but each driver decides).
    ///
    /// **Replace semantics**: calling `add_search` again wipes the previous
    /// search filter before applying the new one. Default produces
    /// `Unimplemented` (when `can_search: true`) or `Unsupported` (when
    /// `can_search: false`).
    fn add_search(&mut self, _text: &str) -> Result<()> {
        Err(self.default_error("add_search", "can_search"))
    }

    /// Drop the search filter previously applied via
    /// [`add_search`](Self::add_search). Default mirrors `add_search`.
    fn clear_search(&mut self) -> Result<()> {
        Err(self.default_error("clear_search", "can_search"))
    }

    // ---- Ordering ----------------------------------------------------------

    /// Push a single ORDER BY clause onto the wrapped table.
    ///
    /// Vista's `add_order` is replace-semantics: the driver shell should clear
    /// any previously-set order before pushing the new one. Default produces
    /// `Unimplemented` (when `can_order: true`) or `Unsupported` (when
    /// `can_order: false`).
    fn add_order(&mut self, _field: &str, _dir: SortDirection) -> Result<()> {
        Err(self.default_error("add_order", "can_order"))
    }

    /// Wipe every order clause. Default mirrors [`add_order`](Self::add_order).
    fn clear_orders(&mut self) -> Result<()> {
        Err(self.default_error("clear_orders", "can_order"))
    }

    // ---- References --------------------------------------------------------

    /// Resolve a same-persistence relation using a known source row, returning
    /// the related table as a new `Vista`.
    ///
    /// Drivers override by forwarding into the wrapped typed `Table`'s
    /// `get_ref_from_row::<EmptyEntity>(relation, &native_row)` and then
    /// wrapping the result back as a `Vista` through the driver's factory.
    /// The default returns `Unimplemented`. Cross-persistence refs are
    /// handled one layer up by `vantage-vista-factory`'s `VistaCatalog`,
    /// never here.
    fn get_ref(&self, relation: &str, _row: &Record<CborValue>) -> Result<Vista> {
        Err(error!(
            format!(
                "get_ref not implemented for '{}'",
                std::any::type_name::<Self>()
            ),
            method = "get_ref",
            relation = relation,
            source_type = std::any::type_name::<Self>()
        )
        .mark_unimplemented()
        .traced())
    }

    /// Build the **bare** target of a same-persistence relation as a `Vista` —
    /// the table a new related row would be inserted into, with no join
    /// condition applied. Used by Vista's nested insert to reach a has-one /
    /// has-many child's destination.
    ///
    /// Drivers override by forwarding into the wrapped typed `Table`'s
    /// `get_ref_target::<EmptyEntity>(relation)` and wrapping the result back
    /// through the driver's factory — the same path as [`get_ref`](Self::get_ref)
    /// minus the row-derived condition. The default returns `Unimplemented`;
    /// cross-persistence relations are rejected at the `Vista` layer before
    /// this is reached.
    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        Err(error!(
            format!(
                "get_ref_target not implemented for '{}'",
                std::any::type_name::<Self>()
            ),
            method = "get_ref_target",
            relation = relation,
            source_type = std::any::type_name::<Self>()
        )
        .mark_unimplemented()
        .traced())
    }

    /// Contained (embedded-in-row) relations this shell exposes, keyed by name.
    /// Default empty — only shells that model embedded objects/arrays override.
    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        static EMPTY: std::sync::OnceLock<IndexMap<String, ContainedSpec>> =
            std::sync::OnceLock::new();
        EMPTY.get_or_init(IndexMap::new)
    }

    /// Resolve a contained relation against a known parent `row`, returning the
    /// embedded records as a sub-`Vista`. Writes to that sub-Vista patch the
    /// host column of `row`'s record back through the shell. Default returns
    /// `Unimplemented`; shells override to seed [`crate::build_contained_vista`]
    /// with a writeback that patches the parent.
    fn get_contained_ref(&self, relation: &str, _row: &Record<CborValue>) -> Result<Vista> {
        Err(error!(
            format!(
                "get_contained_ref not implemented for '{}'",
                std::any::type_name::<Self>()
            ),
            method = "get_contained_ref",
            relation = relation,
            source_type = std::any::type_name::<Self>()
        )
        .mark_unimplemented()
        .traced())
    }

    /// Names + cardinalities of the shell's same-persistence references.
    /// Derived from [`references`](Self::references) by default; impls
    /// should rarely need to override.
    fn get_ref_kinds(&self) -> Vec<(String, crate::reference::ReferenceKind)> {
        self.references()
            .iter()
            .map(|(name, r)| (name.clone(), r.kind))
            .collect()
    }

    // ---- Identity ----------------------------------------------------------

    /// Short human label for the underlying driver (e.g. `"csv"`, `"sqlite"`,
    /// `"postgres"`, `"mongodb"`). Used for diagnostics and CLI output.
    /// Drivers should override; the default is a placeholder.
    fn driver_name(&self) -> &'static str {
        "unknown"
    }

    // ---- Scripting ---------------------------------------------------------

    /// Contribute backend-specific vocabulary to a Rhai engine that
    /// vantage-vista has already seeded with the conventional `Vista` verbs
    /// (see the `rhai_conventional` module). Backends with an expression engine
    /// (SurrealDB, SQL) override this to register `ident`/`==`/`fx`/graph
    /// constructors plus a `with_condition(<backend expr>)` builder that routes
    /// a boxed native condition through [`add_raw_condition`](Self::add_raw_condition).
    ///
    /// Default is a no-op: engine-less datasources (CSV/Mongo/REST) still get
    /// the conventional verbs and only lose the vendor expression syntax —
    /// graceful degradation, not all-or-nothing.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, _engine: &mut rhai::Engine) {}

    // ---- Capability advertisement -----------------------------------------

    fn capabilities(&self) -> &VistaCapabilities;

    /// Look up a capability flag by name. Used by `default_error` to decide
    /// between `Unsupported` and `Unimplemented`. Drivers don't normally
    /// need to override this.
    fn capability_flag(&self, name: &str) -> bool {
        let caps = self.capabilities();
        match name {
            "can_count" => caps.can_count,
            "can_insert" => caps.can_insert,
            "can_update" => caps.can_update,
            "can_delete" => caps.can_delete,
            "can_subscribe" => caps.can_subscribe,
            "can_invalidate" => caps.can_invalidate,
            "can_order" => caps.can_order,
            "can_search" => caps.can_search,
            "can_set_page_size" => caps.can_set_page_size,
            "can_fetch_page" => caps.can_fetch_page,
            "can_fetch_next" => caps.can_fetch_next,
            "can_traverse_to_record" => caps.can_traverse_to_record,
            "can_traverse_to_set" => caps.can_traverse_to_set,
            "can_build_ref_via_script" => caps.can_build_ref_via_script,
            _ => false,
        }
    }

    /// Build the standard error returned by default trait method impls.
    ///
    /// Picks the kind based on the capability flag: a `true` flag means the
    /// driver advertised support but didn't override the method (placeholder
    /// → `Unimplemented`); a `false` flag means the driver honestly doesn't
    /// claim the op (caller should have checked → `Unsupported`).
    ///
    /// Both kinds emit a `tracing::error!` at construction with `method`,
    /// `capability`, `source_type`, and `vista_name` as structured fields.
    fn default_error(&self, method: &str, capability: &str) -> VantageError {
        let source_type = std::any::type_name::<Self>();
        if self.capability_flag(capability) {
            error!(
                format!(
                    "'{}' is advertised as VistaCapability for '{}' but implementation for '{}' is missing",
                    capability, source_type, method
                ),
                method = method,
                capability = capability,
                source_type = source_type
            )
            .mark_unimplemented().traced()
        } else {
            error!(
                format!(
                    "'{}' is not supported by '{}'; '{}' refused",
                    capability, source_type, method
                ),
                method = method,
                capability = capability,
                source_type = source_type
            )
            .mark_unsupported()
            .traced()
        }
    }
}
