use std::ops::Range;

/// Internal bus message published by a Dio for its Sceneries (and any
/// curious user callbacks) to consume.
///
/// Distinct from [`crate::ops::ChangeEvent`] — `ChangeEvent` is the
/// *upstream* shape (what a SurrealDB LIVE stream or a webhook delivers
/// about the master backend), while `DioEvent` is the *internal* fanout
/// shape Sceneries react to.
#[derive(Debug, Clone)]
pub enum DioEvent {
    RecordChanged {
        id: String,
    },
    RecordInserted {
        id: String,
    },
    RecordRemoved {
        id: String,
    },
    Invalidated,
    Refreshing,
    WriteFailed {
        id: Option<String>,
        error: String,
    },

    /// An optimistic write was just staged in the cache: the new value is
    /// already visible, but the write-through hasn't confirmed yet. Sceneries
    /// flip the row for `id` to [`PendingWrite`](crate::RowStatus::PendingWrite).
    WritePending {
        id: String,
    },

    /// An optimistic write failed and its cache pre-image was restored. The
    /// value has reverted; sceneries surface the error by flipping the row for
    /// `id` to [`WriteFailed`](crate::RowStatus::WriteFailed). Distinct from
    /// [`WriteFailed`](Self::WriteFailed), the fire-and-forget facade-queue
    /// failure that does not touch the cache.
    WriteReverted {
        id: String,
        error: String,
    },

    /// Emitted by `TableScenery` once a `set_viewport` / `request_load_more`
    /// has cleared its debounce window and committed a viewport. Always
    /// fires; a viewport entirely inside the cached range still emits this
    /// (with no follow-up `RangeLoaded`).
    ViewportChanged {
        range: Range<usize>,
    },

    /// Emitted by `TableScenery` after `on_load_chunk` returns `Ok`. The
    /// `range` carries the indices the callback actually requested — the
    /// callback may have pushed fewer rows.
    RangeLoaded {
        range: Range<usize>,
    },

    /// Emitted by `TableScenery` when `on_load_chunk` returns `Err`. The
    /// sparse map is left untouched; the slots in `range` stay whatever
    /// they were before the attempt.
    LoadFailed {
        range: Range<usize>,
        error: String,
    },
}
