//! [`TableShell`]: the per-driver executor behind a [`Vista`].

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
#[cfg(feature = "rhai")]
use vantage_rhai::rhai;
use vantage_types::Record;

use crate::{
    capabilities::{Capability, VistaCapabilities, unsupported},
    change::VistaChangeStream,
    metadata::VistaMetadata,
    page::{NativeCondition, Page, Preview},
    query::Condition,
    sort::SortDirection,
    vista::Vista,
};

type Rec = Record<CborValue>;

/// The driver side of a [`Vista`]: schema, reads, narrowing and writes over
/// one table of one backend.
///
/// A shell owns its query state. `Vista` narrows an owned clone through the
/// `&mut self` methods, so a shell's `Clone` must carry its current narrowing
/// and own it independently, while sharing the store or connection. Every
/// read and write runs over the shell's current narrowing.
///
/// A method with a default answers [`unsupported`] with the capability that
/// names it; a shell that implements the method advertises that capability in
/// [`capabilities`](Self::capabilities). See [`Capability`] for the mapping.
///
/// Ids are strings at this boundary, and values are CBOR: every driver's
/// native id (Mongo `ObjectId`, Surreal `Thing`, …) stringifies here.
#[async_trait]
pub trait TableShell: CloneTableShell + Send + Sync + 'static {
    // ---- Schema -------------------------------------------------------------

    /// The set's columns, references and id column. `Vista` reads its
    /// metadata vocabulary (`columns`, `column`, `id_column`, …) from here.
    fn metadata(&self) -> &VistaMetadata;
    fn capabilities(&self) -> &VistaCapabilities;
    /// Short label for the driver (`"csv"`, `"sqlite"`, `"mongodb"`).
    fn driver_name(&self) -> &'static str;

    /// The query the next read would send, rendered without sending it.
    /// Synchronous and performs no I/O: a value known only at fetch time
    /// renders as a placeholder. Lossy is acceptable, wrong is not.
    /// See [`Preview`] for what `query` and `note` each hold.
    fn preview(&self) -> Preview;

    // ---- Reads --------------------------------------------------------------

    /// Every row of the set, keyed by id, in the set's order.
    async fn list(&self) -> Result<IndexMap<String, Rec>>;
    /// Row `id` when it is in the set.
    async fn get(&self, id: &str) -> Result<Option<Rec>>;
    async fn count(&self) -> Result<i64>;
    /// Rows `[offset, offset + limit)` of the set, in its order.
    ///
    /// The leading window (`offset == 0`) is required of every shell whose
    /// [`list`](Self::list) works: it is how a capped read and
    /// `get_some_value` read. A shell that can't push a window to its
    /// backend answers it with [`Page::from_listing`] over its listed rows.
    /// [`Window`](Capability::Window) means any offset; `Vista` refuses
    /// `offset > 0` without it before the shell is called.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page>;

    /// Up to `limit` rows after `token` (`None` starts at the beginning) and
    /// the token for the next call, `None` once the set is exhausted
    /// ([`Cursor`](Capability::Cursor)). The token is driver-private; callers
    /// hand it back unchanged. `Vista` refuses the call without `Cursor`
    /// before the shell is called.
    async fn fetch_cursor(
        &self,
        _token: Option<CborValue>,
        _limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        unsupported(Capability::Cursor)
    }

    /// Subscribe to changes on the set ([`Subscribe`](Capability::Subscribe)).
    ///
    /// Delivery is best-effort, and four promises hold for every driver:
    /// 1. the stream may be coarser than the set (table-wide is valid);
    /// 2. so a payload row may fall outside the set: either the driver
    ///    reconciles (re-reads the id through the set, reporting `Deleted`)
    ///    or the consumer must;
    /// 3. [`Invalidated`](crate::VistaChange::Invalidated) means "re-read
    ///    everything";
    /// 4. the stream ending is normal; consumers resubscribe and reconcile.
    ///
    /// Advertise `Subscribe` only when the driver pushes with no further
    /// setup, or the application declared the setup done: a stream that stays
    /// silent can't be told apart from nothing happening. `Vista` refuses
    /// the call without `Subscribe` before the shell is called.
    async fn watch(&self) -> Result<VistaChangeStream> {
        unsupported(Capability::Subscribe)
    }

    // ---- Narrowing ----------------------------------------------------------
    //
    // Error kinds matter here: `Vista::with_query` keeps a clause for
    // local evaluation only when it is refused as `Unsupported` (or
    // `Unimplemented`). A clause the shell could take but finds wrong (an
    // unknown column, a value it can't convert) answers a normal error, which
    // the caller sees. A shell that errors leaves its narrowing unchanged.

    /// Narrow the set by `condition`. Every shell takes equality; `Vista`
    /// refuses other operators unless the shell advertises
    /// [`FilterOps`](Capability::FilterOps). An unknown column or a bad value
    /// is a normal error, not `Unsupported`.
    ///
    /// A dotted column (`root.leaf`) names a field nested in a declared
    /// root's value. The shell either evaluates it (narrowing to the rows
    /// whose `root` holds a matching `leaf`) or rejects it with a normal
    /// error. It never accepts the path and matches nothing.
    fn add_condition(&mut self, condition: &Condition) -> Result<()>;

    /// Narrow the set by a condition in the driver's own type
    /// ([`NativeConditions`](Capability::NativeConditions)). A condition of
    /// a type the driver doesn't take is an `IncorrectUsage` error.
    fn add_native_condition(&mut self, _condition: NativeCondition) -> Result<()> {
        unsupported(Capability::NativeConditions)
    }

    /// Replace the quicksearch text; `None` clears it
    /// ([`Search`](Capability::Search)). The shell decides which columns the
    /// text is matched against, usually those flagged `searchable`. Text the
    /// backend can't express is a normal error, not `Unsupported`.
    fn set_search(&mut self, _text: Option<&str>) -> Result<()> {
        unsupported(Capability::Search)
    }

    /// Replace the order; `None` clears it ([`Order`](Capability::Order)).
    /// An unknown column is a normal error, not `Unsupported`.
    fn set_order(&mut self, _order: Option<(&str, SortDirection)>) -> Result<()> {
        unsupported(Capability::Order)
    }

    // ---- Writes -------------------------------------------------------------
    //
    // Writes follow the write contract on `vantage_dataset::WritableValueSet`
    // over the shell's narrowing. A shell that keeps every write inside a
    // narrowed set advertises `ConfineWrites`; `Vista` refuses writes through
    // a narrowed handle whose shell doesn't.

    /// Insert `record` as row `id`, or under an id the backend makes when
    /// `id` is `None` (not retry-safe). Returns the id and the stored row.
    /// An id already in the set returns the stored row untouched; an id held
    /// outside the set is `Conflict`; a new row is conformed to the set's
    /// equality conditions.
    async fn insert(&self, _id: Option<&str>, _record: &Rec) -> Result<(String, Rec)> {
        unsupported(Capability::Insert)
    }

    /// Replace row `id`, creating it when missing; an id held outside the set
    /// is `Conflict`.
    async fn replace(&self, _id: &str, _record: &Rec) -> Result<Rec> {
        unsupported(Capability::Update)
    }

    /// Insert row `id`, or replace it when it exists, in one operation.
    async fn upsert(&self, _id: &str, _record: &Rec) -> Result<Rec> {
        unsupported(Capability::Update)
    }

    /// Merge `partial` into row `id`. A row outside the set or missing is
    /// `NotFound`; a result that leaves the set is `Conflict`.
    async fn patch(&self, _id: &str, _partial: &Rec) -> Result<Rec> {
        unsupported(Capability::Update)
    }

    /// Remove row `id` when it is in the set; `Ok` whether it was in the set,
    /// outside it, or missing.
    async fn delete(&self, _id: &str) -> Result<()> {
        unsupported(Capability::Delete)
    }

    /// Remove exactly the rows in the set.
    async fn delete_all(&self) -> Result<()> {
        unsupported(Capability::Delete)
    }

    /// Store `rows` in one all-or-nothing, driver-native operation and return
    /// how many were newly inserted; an id already held counts zero
    /// ([`Import`](Capability::Import)). A driver that can't make the batch
    /// atomic, or can't tell new rows from existing ones, doesn't advertise
    /// `Import`; callers then insert row by row.
    async fn import(&self, _rows: &IndexMap<String, Rec>) -> Result<usize> {
        unsupported(Capability::Import)
    }

    // ---- References ---------------------------------------------------------

    /// The target of `relation` ([`Traverse`](Capability::Traverse)): with a
    /// parent `row` (from this shell's set), the related rows; with `None`,
    /// the bare target a new related row is inserted into. Contained
    /// relations route through here too: from a row, the records it embeds;
    /// with `None`, the contained schema with no records and no write
    /// capabilities, since there is no parent row to write into.
    ///
    /// Narrow a target for a row through `Vista::add_condition` (or
    /// `with_condition`, `with_native_condition`), never through the target
    /// shell directly, so that the returned Vista's `query()` records the
    /// join and `is_narrowed()` holds.
    fn get_ref(&self, _relation: &str, _row: Option<&Rec>) -> Result<Vista> {
        unsupported(Capability::Traverse)
    }

    // ---- Scripting ----------------------------------------------------------

    /// Register backend vocabulary on a Rhai engine alongside vantage-vista's
    /// `DataVocab`: expression constructors, a `with_condition(<expr>)` verb
    /// routed through [`add_native_condition`](Self::add_native_condition),
    /// and any per-evaluation constants. A backend with no expression engine
    /// registers nothing; scripts still get the table handle verbs.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, _engine: &mut rhai::Engine) {}
}

/// Boxed cloning for [`TableShell`], so `Box<dyn TableShell>` is `Clone`.
/// Every `TableShell + Clone` has it; a shell implements `Clone` and gets
/// this for free. The trait is sealed: a clone must come from `Clone`, so
/// it carries the shell's narrowing.
pub trait CloneTableShell: sealed::Sealed {
    #[doc(hidden)]
    fn clone_boxed(&self) -> Box<dyn TableShell>;
}

mod sealed {
    pub trait Sealed {}
    impl<T: super::TableShell + Clone> Sealed for T {}
}

impl<T: TableShell + Clone> CloneTableShell for T {
    fn clone_boxed(&self) -> Box<dyn TableShell> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn TableShell> {
    fn clone(&self) -> Self {
        (**self).clone_boxed()
    }
}
