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
}
