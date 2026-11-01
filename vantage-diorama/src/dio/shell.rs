use std::sync::Arc;

use vantage_core::Result;
use vantage_vista::{Capability, ColumnFlag, Vista, VistaCapabilities, VistaMetadata, VistaQuery};

use super::DioInner;

mod preview;
mod reads;
mod watch;
mod writes;

/// Where a facade read takes its rows from, and what it evaluates over them.
pub(crate) struct ReadPlan {
    /// The master narrowed by the clauses it took, or `None` to read the
    /// cache.
    pub(crate) source: Option<Vista>,
    /// The handle's clauses `source` doesn't reflect. The Dio's own residual
    /// is applied on top of these by every read.
    pub(crate) local: VistaQuery,
}

/// `TableShell` impl that backs the Vista returned by `Dio::cached()`.
///
/// Holds the shared inner Dio state so reads and writes route through the
/// Dio's machinery. Schema (columns, refs, id) is copied from the master at
/// construction, every column marked orderable. The facade advertises only
/// what it implements itself; see `DioShell::new`.
#[derive(Clone)]
pub(crate) struct DioShell {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// This handle's narrowing. Per-handle, and cloned rather than shared, so
    /// narrowing one facade never reorders another's view of the same Dio.
    pub(crate) query: VistaQuery,
}

impl DioShell {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        let master = &dio.master;
        // Every column becomes orderable, whatever the master says.
        // `Vista::set_order` refuses a column without the `Orderable` flag, and
        // most drivers only set it for columns their engine can sort (CSV sets
        // it for none). The facade sorts over the rows it reads when the
        // master can't, so refusing here would deny a sort it can perform.
        let mut metadata = VistaMetadata::new();
        metadata.columns = master
            .columns()
            .iter()
            .map(|(name, column)| {
                (
                    name.clone(),
                    column.clone().with_flag(ColumnFlag::Orderable),
                )
            })
            .collect();
        metadata.references = master.references().clone();
        metadata.id_column = Some(dio.id_column.clone());
        metadata.id_generator = master.id_generator().cloned();
        metadata.denials = master.denials();
        let write_caps = dio.write_capabilities();

        // The facade advertises what `DioShell` itself answers, and nothing
        // it would refuse:
        // - writes follow `Dio::write_capabilities` (master OR Lens writer,
        //   never a denied one), and every write keeps to the handle's
        //   narrowing; the master's denials ride along in the metadata;
        // - `Subscribe`: the event bus backs `watch`;
        // - `Search`, `FilterOps`, `Order`: pushed into the master when it
        //   takes them, evaluated over the rows read when it doesn't;
        // - `Window`: windowed over the rows read (hydrating augment gaps).
        //
        // Not advertised: `Import` (bulk import goes through
        // `Dio::import_values`), `Traverse` (traverse through `Dio::get_ref`),
        // `Cursor` and `NativeConditions`.
        let capabilities = [
            (write_caps.can_insert, Capability::Insert),
            (write_caps.can_update, Capability::Update),
            (write_caps.can_delete, Capability::Delete),
            (write_caps.can_delete_all, Capability::DeleteAll),
            (true, Capability::Subscribe),
            (true, Capability::Search),
            (true, Capability::FilterOps),
            (true, Capability::Order),
            (true, Capability::Window),
            (true, Capability::ConfineWrites),
        ]
        .into_iter()
        .filter_map(|(on, capability)| on.then_some(capability))
        .collect();
        Self {
            dio,
            capabilities,
            metadata,
            query: VistaQuery::new(),
        }
    }

    /// A handle over `dio` narrowed by `query`, with `metadata` and no
    /// capabilities: enough to read the narrowed set and nothing else.
    fn reader(dio: Arc<DioInner>, query: VistaQuery, metadata: VistaMetadata) -> Self {
        Self {
            dio,
            capabilities: VistaCapabilities::default(),
            metadata,
            query,
        }
    }

    /// Whether this handle's own clauses limit its rows. Order doesn't.
    pub(crate) fn narrows(&self) -> bool {
        !self.query.conditions.is_empty() || self.query.search.is_some()
    }

    /// Route this handle's query through the Dio's one routing step,
    /// [`DioInner::plan`]. Every read method takes its source from here.
    ///
    /// 1. **A handle that doesn't narrow reads the cache.** An order on its
    ///    own never moves a read off the cache, so sorting never changes
    ///    which rows a handle shows; the order is applied over the cache.
    /// 2. **An augmented Dio always reads its cache.** Augment columns exist
    ///    only there (the master has never heard of them), so both a filter
    ///    on one and the values of one would be lost by reading the master.
    /// 3. Otherwise the master takes what it can. When it took a condition
    ///    or the search, the read goes to the narrowed master, whose answer
    ///    covers the whole set rather than whatever the cache holds, and the
    ///    refused clauses are evaluated over it. When it took neither, the
    ///    cache is read.
    ///
    /// A cache read evaluates the handle's whole query. A clause the master
    /// rejects outright (an unknown column) is returned as the error.
    pub(crate) fn plan(&self) -> Result<ReadPlan> {
        let from_cache = || ReadPlan {
            source: None,
            local: self.query.clone(),
        };
        if !self.narrows() || self.dio.has_dio_augment() {
            return Ok(from_cache());
        }
        let super::Plan { narrowed, refused } = self.dio.plan(&self.query)?;
        let pushed = refused.conditions.len() < self.query.conditions.len()
            || (self.query.search.is_some() && refused.search.is_none());
        if !pushed {
            return Ok(from_cache());
        }
        Ok(ReadPlan {
            source: Some(narrowed),
            local: refused,
        })
    }

    /// Whether evaluating `local` and the Dio's residual over cached rows
    /// needs their augmented values; see [`DioInner::names_augmented`].
    pub(crate) fn needs_hydration(&self, local: &VistaQuery) -> bool {
        self.dio.names_augmented(local)
    }
}
