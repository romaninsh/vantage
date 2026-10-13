use std::ops::Range;

/// Internal bus message published by a Dio for its Sceneries (and any
/// curious user callbacks) to consume.
///
/// Distinct from [`crate::ops::ChangeEvent`] — `ChangeEvent` is the
/// *upstream* shape (what a SurrealDB LIVE stream or a webhook delivers
/// about the master backend), while `DioEvent` is the *internal* fanout
/// shape Sceneries react to.
/// Marked `#[non_exhaustive]`: the bus gains a variant whenever the layer
/// learns to say something new about a load, and a consumer that cares about
/// three of them should not break when a fourth appears. Every match on this
/// enum needs a fallback arm, which is the right default anyway — an event you
/// have not heard of is one you have no handling for.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    DatasetChanged,

    /// The lens's own eager start has finished writing the whole
    /// set to the cache.
    ///
    /// Distinct from [`DatasetChanged`](Self::DatasetChanged), which says only
    /// "membership moved — re-derive however your shape demands". This says
    /// what landed and where it is: the full set, in the cache, now. Every
    /// single-pass scenery answers it the same way — reseed the visible map
    /// from the cache — whatever its load shape.
    ///
    /// That uniformity is the point. Announced as `DatasetChanged`, the seed's
    /// arrival became a question about shape, and a scenery under a lens that
    /// also offers a paged loader answered it wrongly: re-fetch the viewport
    /// through a pager it never pages through. The rows sat in the cache and
    /// the grid stayed empty. A scenery cannot get this one wrong by
    /// misjudging its own shape, because it does not consult it.
    Seeded,

    Refreshing,

    /// A write with no row to stage (an id-less insert, a `Clear`) failed.
    /// The cache is untouched; the caller of
    /// [`Dio::flash`](crate::Dio::flash) also receives the error.
    WriteFailed {
        id: Option<String>,
        error: String,
    },

    /// An optimistic write was just staged in the cache: the new value is
    /// already visible, but the write-through hasn't confirmed yet. Sceneries
    /// flip the row for `id` to [`PendingWrite`](crate::RowStatus::PendingWrite).
    WritePending {
        id: String,
        /// What the staged flash is doing — see
        /// [`WriteReverted`](Self::WriteReverted) for why listeners filter
        /// on this.
        kind: crate::FlashKind,
    },

    /// An optimistic write failed and its cache pre-image was restored. The
    /// value has reverted; sceneries surface the error by flipping the row for
    /// `id` to [`WriteFailed`](crate::RowStatus::WriteFailed). Distinct from
    /// [`WriteFailed`](Self::WriteFailed), a failed write that staged no row.
    WriteReverted {
        id: String,
        error: String,
        /// What the failed flash was doing. Lets a listener decide whether
        /// the failure is its own business: an edit form's servo reports
        /// Patch/Replace/Insert reverts, but a failed Delete belongs to
        /// its issuer (the confirm dialog), not to a form that happens to
        /// display the same record.
        kind: crate::FlashKind,
    },

    /// Emitted by `TableScenery` once a `set_viewport` / `request_load_more`
    /// has cleared its debounce window and committed a viewport. Always
    /// fires; a viewport entirely inside the cached range still emits this
    /// (with no follow-up `RangeLoaded`).
    ViewportChanged {
        range: Range<usize>,
    },

    /// Emitted by `TableScenery` after the paged loader returns `Ok`. The
    /// `range` carries the indices the callback actually requested — the
    /// callback may have pushed fewer rows.
    RangeLoaded {
        range: Range<usize>,
    },

    /// Emitted by `TableScenery` when the paged loader returns `Err`. The
    /// sparse map is left untouched; the slots in `range` stay whatever
    /// they were before the attempt.
    ///
    /// Also emitted, with an empty `range`, when a live follow fails to
    /// re-read the set after a dropped subscription or a change that failed
    /// to apply; the follow retries until a re-read succeeds.
    LoadFailed {
        range: Range<usize>,
        error: String,
    },

    /// A scheduled per-row detail fetch failed. Distinct from
    /// [`LoadFailed`](Self::LoadFailed) (a chunk/list page that couldn't
    /// load, addressed by range): detail fetches run centrally in the
    /// augment scheduler, which knows the row's id but not any scenery's
    /// index for it — each two-pass scenery that holds the id stamps its
    /// own slot [`LoadFailed`](crate::RowStatus::LoadFailed).
    RecordLoadFailed {
        id: String,
        error: String,
    },

    /// A facade-Vista read found rows whose augment columns aren't
    /// hydrated yet and is about to fetch them one by one. Fires once
    /// per read, before the first fetch — the read blocks until
    /// hydration completes, so this is the consumer's cue to tell the
    /// user what's coming ("fetching 1122 files…"). Each hydrated row
    /// then emits [`RecordChanged`](Self::RecordChanged) for progress.
    Hydrating {
        pending: usize,
    },
}
