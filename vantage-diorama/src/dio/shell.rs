use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_vista::{Column, Reference, SortDirection, Vista, VistaCapabilities, flags};

use super::DioInner;

/// Narrowing applied to a facade handle.
///
/// Held whole rather than pre-split, because whether a clause can be pushed
/// into the master is decided at read time by *trying* it — capability flags
/// describe a driver in general, not whether it accepts one particular column.
#[derive(Default, Clone)]
pub(crate) struct FacadeQuery {
    pub(crate) conditions: Vec<(String, CborValue)>,
    pub(crate) order: Option<(String, SortDirection)>,
}

impl FacadeQuery {
    pub(crate) fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.order.is_none()
    }
}

/// `TableShell` impl that backs the Vista returned by `Dio::vista()`.
///
/// Holds the shared inner Dio state so reads/writes route through the
/// Dio's machinery. Schema (columns, refs, id) is **snapshotted** from the
/// master at construction so the facade doesn't borrow the now-swappable
/// master across reads; a fresh `dio.vista()` after a [`reload`](crate::Dio::reload)
/// captures the new schema. Capability advertisement is the union of the
/// master's capabilities and what the Lens's callbacks unlock.
pub struct DioShell {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) columns: IndexMap<String, Column>,
    pub(crate) references: IndexMap<String, Reference>,
    pub(crate) id_column: Option<String>,
    /// This handle's narrowing. Per-handle, and cloned rather than shared, so
    /// narrowing one facade never reorders another's view of the same Dio.
    pub(crate) query: FacadeQuery,
}

impl DioShell {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        let master = dio.master.read().unwrap();
        let master_caps = master.capabilities().clone();
        // Every column becomes orderable, whatever the master says.
        // `Vista::add_order` refuses a column without the `ORDERABLE` flag, and
        // most drivers only set it for columns their *engine* can sort — CSV
        // sets it for none. But the facade can always fall back to ordering the
        // cache, so refusing here would deny a sort the Dio is perfectly able
        // to perform. `SEARCHABLE` gets no such treatment: the facade has no
        // search of its own to fall back on.
        let columns = master
            .source
            .columns()
            .iter()
            .map(|(name, column)| {
                let mut column = column.clone();
                if !column.has_flag(flags::ORDERABLE) {
                    column = column.with_flag(flags::ORDERABLE);
                }
                (name.clone(), column)
            })
            .collect();
        let references = master.source.references().clone();
        let id_column = master.source.id_column().map(str::to_string);
        drop(master);
        let has_on_event = dio.lens.callbacks.on_event.is_some();
        let write_caps = dio.write_capabilities();

        // Capability lifting rules (architecture doc):
        //   can_insert/update/delete = Dio::write_capabilities (master OR route)
        //   can_subscribe            = always true (Dio fans out events)
        //   can_invalidate           = master.can_invalidate OR on_event registered
        //   can_count                = always true (cache answers locally)
        let capabilities = VistaCapabilities {
            can_count: true,
            can_insert: write_caps.can_insert,
            can_update: write_caps.can_update,
            can_delete: write_caps.can_delete,
            // Bulk import goes through `Dio::import_values`, which picks
            // the master's native path or the per-record fallback itself —
            // the facade never takes an import.
            can_import: false,
            can_subscribe: true,
            can_invalidate: master_caps.can_invalidate || has_on_event,
            // Ordering is always available: the facade pushes it into the
            // master when it can answer it, and orders the cache itself when it
            // cannot. Advertising the master's flag here used to promise what
            // `DioShell` then refused — the flag was true and the method
            // `Unimplemented`.
            can_order: true,
            // Search is NOT lifted. The facade has no search of its own, so it
            // can only honour one the master answers — and it has nowhere to
            // forward the term to, since narrowing is replayed at read time.
            // Advertising `true` here would promise a filter that never ran and
            // hand back the unfiltered set. Narrow the master vista directly
            // for a server-side search.
            can_search: false,
            // Whether operator filters (`!=`, `<`, `in`, …) can be pushed into
            // the master's query. When false the Dio still filters locally over
            // the cache — the caller just can't bake the condition into a
            // distinct cached query variant.
            can_filter_operators: master_caps.can_filter_operators,
            can_set_page_size: master_caps.can_set_page_size,
            can_fetch_page: master_caps.can_fetch_page,
            can_fetch_next: master_caps.can_fetch_next,
            // The facade windows over the cache locally (and hydrates the
            // window's augment gaps), regardless of what the master can do.
            can_fetch_window: true,
            // Traversal capabilities pass through from the master vista; the
            // Dio cache does not add or remove traversal modes.
            can_traverse_to_record: master_caps.can_traverse_to_record,
            can_traverse_to_set: master_caps.can_traverse_to_set,
            can_build_ref_via_script: master_caps.can_build_ref_via_script,
            // Column traversal is lowered into the master's query; the cache
            // passes it through unchanged.
            can_traverse_in_columns: master_caps.can_traverse_in_columns,
        };
        Self {
            dio,
            capabilities,
            columns,
            references,
            id_column,
            query: FacadeQuery::default(),
        }
    }

    /// Split this handle's narrowing into "the master will do it" and "we do it
    /// over the cache", and build the narrowed master if anything pushed down.
    ///
    /// The rule, in order:
    ///
    /// 1. **An augmented Dio always reads its cache.** Augment columns exist
    ///    only there — the master has never heard of them — so both a filter on
    ///    one and the *values* of one would be lost by reading the master. Every
    ///    clause stays local.
    /// 2. Otherwise each clause is offered to a private clone of the master.
    ///    Whatever it accepts is answered at the source, authoritatively over
    ///    the whole set rather than over whatever the cache happens to hold.
    /// 3. Whatever it refuses is applied here.
    ///
    /// Clauses are *tried* rather than predicted from capability flags: a flag
    /// describes a driver, not whether it accepts one particular column.
    pub(crate) fn plan(&self) -> (Option<Vista>, FacadeQuery) {
        if self.query.is_empty() {
            return (None, FacadeQuery::default());
        }
        if self.dio.is_two_pass() {
            return (None, self.query.clone());
        }

        let master = self.dio.master.read().unwrap();
        let Some(shell) = master.source.clone_shell() else {
            // A master that cannot be cloned cannot be narrowed privately, and
            // narrowing the shared one would leak into every other reader.
            return (None, self.query.clone());
        };
        let mut narrowed = Vista::new(master.name(), shell);
        drop(master);

        let mut local = FacadeQuery::default();
        let mut pushed = false;

        for (field, value) in &self.query.conditions {
            if narrowed
                .add_condition_eq(field.clone(), value.clone())
                .is_ok()
            {
                pushed = true;
            } else {
                local.conditions.push((field.clone(), value.clone()));
            }
        }
        if let Some((column, direction)) = &self.query.order {
            if narrowed.add_order(column, *direction).is_ok() {
                pushed = true;
            } else {
                local.order = Some((column.clone(), *direction));
            }
        }
        (pushed.then_some(narrowed), local)
    }
}
