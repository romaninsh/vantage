use std::sync::Arc;

use indexmap::IndexMap;
use vantage_vista::{Column, Reference, VistaCapabilities};

use super::DioInner;

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
}

impl DioShell {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        let master = dio.master.read().unwrap();
        let master_caps = master.capabilities().clone();
        let columns = master.source.columns().clone();
        let references = master.source.references().clone();
        let id_column = master.source.id_column().map(str::to_string);
        drop(master);
        let cbs = &dio.lens.callbacks;
        let has_on_write = cbs.on_write.is_some();
        let has_on_event = cbs.on_event.is_some();

        // Capability lifting rules (architecture doc):
        //   can_insert/update/delete = master.X OR on_write registered
        //   can_subscribe            = always true (Dio fans out events)
        //   can_invalidate           = master.can_invalidate OR on_event registered
        //   can_count                = always true (cache answers locally)
        let capabilities = VistaCapabilities {
            can_count: true,
            can_insert: master_caps.can_insert || has_on_write,
            can_update: master_caps.can_update || has_on_write,
            can_delete: master_caps.can_delete || has_on_write,
            can_subscribe: true,
            can_invalidate: master_caps.can_invalidate || has_on_event,
            // Read-side query controls reflect the cache today. Stage 5b
            // will swap in cache-aware truth for these.
            can_order: master_caps.can_order,
            can_search: master_caps.can_search,
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
        }
    }
}
