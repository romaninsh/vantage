use serde::{Deserialize, Serialize};

/// Honest contract a driver advertises to consumers.
///
/// Every flag corresponds to a method on `TableShell` that the driver
/// either implements server-side (flag is `true`) or refuses (flag is
/// `false`). UIs branch on these flags to decide which controls to
/// render; scripted callers branch on them to know whether to call the
/// method at all.
///
/// **Pagination rule**: when both [`can_fetch_page`](Self::can_fetch_page)
/// and [`can_fetch_next`](Self::can_fetch_next) are `false`, the driver
/// has no native pagination — consumers fall through to plain
/// `list_values` which returns everything.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VistaCapabilities {
    pub can_count: bool,
    pub can_insert: bool,
    pub can_update: bool,
    pub can_delete: bool,
    pub can_subscribe: bool,
    pub can_invalidate: bool,
    /// Server-side ordering via `add_order(column, direction)`. When
    /// `true`, individual columns may still refuse — check the per-
    /// column `ORDERABLE` flag.
    pub can_order: bool,
    /// Server-side quicksearch via `add_search(text)`, OR'd across
    /// columns flagged `SEARCHABLE`.
    pub can_search: bool,
    /// Consumer may pick the page size via `set_page_size(n)`. Some
    /// REST APIs return fixed-size pages and set this to `false`.
    pub can_set_page_size: bool,
    /// Random-access pagination via `fetch_page(n)`. Offset-style.
    pub can_fetch_page: bool,
    /// Chain-forward pagination via `fetch_next(token)`. Cursor-style;
    /// the weakest of the three pagination primitives. DynamoDB and
    /// most token-paginated REST APIs only support this.
    pub can_fetch_next: bool,
    /// Record-level reference traversal via `get_ref(relation, row)` — read
    /// the join value out of a known row and narrow the target with a plain
    /// eq-condition. Every backend that can filter by equality supports this
    /// (SQL, CSV, Mongo, Surreal, REST/GraphQL).
    pub can_traverse_to_record: bool,
    /// Set-level reference traversal — narrow the target with an
    /// `IN (subquery)` derived from the parent's own conditions (the
    /// `get_ref_as` / reports path). Requires the backend to support
    /// subqueries; SQL and SurrealDB do, CSV/Mongo/REST do not.
    pub can_traverse_to_set: bool,
    /// Per-reference Rhai-scripted traversal — a reference carrying a
    /// `build_script` resolves through the script engine (the
    /// `TableShell::register_rhai_extensions` hook, available with the `rhai`
    /// feature) rather than the fixed FK eq-condition path. Backends with a script
    /// engine *and* a by-name target resolver advertise `true`; others leave
    /// it `false` and ignore any `build_script` (the FK path still works).
    pub can_build_ref_via_script: bool,
}
