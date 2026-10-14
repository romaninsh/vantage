//! Placing a capped view over its rows, and keeping a tail on the end of
//! its set.
//!
//! A tail counts back from the end of the set, so it can only place its
//! rows once that end is known. Over a complete set it always is. Over
//! windows it is once the source states a total (or a short page proves the
//! end), and the tail then asks for the last `n` rows. A source that pages
//! windows without ever stating a total leaves the end a horizon: the tail
//! turns into a flipped one and reads the first `n` rows of the flipped
//! order instead. Until it knows which, it shows nothing and reads as
//! loading, so it never shows rows from the wrong end.

use std::sync::Weak;
use std::sync::atomic::Ordering;

use tokio::sync::watch;

use crate::dio::Generation;
use crate::scenery::table::LoadFailure;
use crate::scenery::table::cap::Cap;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::view_stats::TotalKind;

use super::TableView;
use super::repoint::Change;

/// How a view reads the rows it points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placing {
    /// No cap: every row.
    Whole,
    /// Through `Cap` over this many shared rows.
    Capped(Cap, usize),
    /// A tail over windows whose end is not known yet: no rows.
    Unplaced(Cap),
}

/// What a tail knows of the end of its rows' set.
enum End {
    /// The set has this many rows.
    Known(usize),
    /// Not yet: the first load hasn't stated it, or the list pass is still
    /// listing.
    Pending,
    /// Never: the source pages without a total.
    Horizon,
}

/// Over two-pass rows the visible map holds the set only once the list
/// pass has listed the index to the end and the refined map has settled.
/// A tail always carries an order, so its two-pass rows always refine, and
/// a refined list pass lists to the end on its own; flipping would only
/// list a second index the same unordered way. A remembered total places a
/// tail over windows provisionally, so its cached rows show, until the
/// source restates it.
fn end_of(shared: &SharedRows) -> End {
    if let Some(index) = shared.index() {
        let settled = shared.settled.load(Ordering::SeqCst);
        return if index.is_complete() && settled {
            End::Known(shared.row_count())
        } else {
            End::Pending
        };
    }
    if !shared.is_chunk_loaded() {
        return End::Known(shared.row_count());
    }
    let total = *shared.total.read().unwrap();
    match (total, *shared.total_kind.read().unwrap()) {
        (Some(total), TotalKind::Exact | TotalKind::Remembered) => End::Known(total),
        (Some(_), TotalKind::Horizon) => End::Horizon,
        (None, _) => End::Pending,
    }
}

/// Whether a set order refused, lifted a refusal, or changed neither.
pub(super) enum Refusal {
    Refused,
    Lifted,
    Unchanged,
}

impl TableView {
    /// How the view reads `shared` right now.
    pub(crate) fn placing(&self, shared: &SharedRows) -> Placing {
        match self.cap() {
            None => Placing::Whole,
            Some(cap @ Cap::Tail(_)) => match end_of(shared) {
                End::Known(len) => Placing::Capped(cap, len),
                End::Pending | End::Horizon => Placing::Unplaced(cap),
            },
            Some(cap) => Placing::Capped(cap, shared.row_count()),
        }
    }

    /// A tail has no last rows without an order: `unordered` fails it, and
    /// the view stays on the rows it has until an order comes back. Leaves a
    /// trace in the debug stream and the log; it is not a load failure, so
    /// no `LoadFailed` goes on the bus. The caller holds `repointing`.
    pub(super) fn refuse_unordered_tail(&self, unordered: bool) -> Refusal {
        if !self.cap().is_some_and(Cap::is_tail) {
            return Refusal::Unchanged;
        }
        let refusal = unordered.then(|| {
            let table = self.dio_weak.upgrade().map(|dio| dio.master.name().to_string());
            LoadFailure::new(table.unwrap_or_default(), "a tail view needs an order")
        });
        if let Some(failure) = &refusal {
            tracing::warn!(
                target: "vantage_diorama::sort",
                table = %failure.table,
                "a tail view needs an order; it shows no rows until one is set",
            );
            crate::debug::tapline!(
                self.shared().debug_tap,
                "sort",
                "refused: a tail view needs an order"
            );
        }
        let was_refused =
            std::mem::replace(&mut *self.refused.write().unwrap(), refusal).is_some();
        if unordered || was_refused {
            self.signal.bump();
            self.signal.bump_stats();
        }
        match (unordered, was_refused) {
            (true, _) => Refusal::Refused,
            (false, true) => Refusal::Lifted,
            (false, false) => Refusal::Unchanged,
        }
    }

    /// Keep a tail on the end of its set: ask for the last `n` rows each
    /// time the end moves, or turn flipped once the end proves a horizon.
    fn place_tail(&self) {
        let Some(Cap::Tail(n)) = self.cap() else {
            return;
        };
        if self.refusal().is_some() {
            return;
        }
        let _serial = self.repointing.lock().unwrap_or_else(|e| e.into_inner());
        let shared = self.shared();
        match end_of(&shared) {
            End::Pending => {}
            End::Known(len) => {
                let mut placed = self.placed_end.lock().unwrap();
                if *placed != Some(len) {
                    *placed = Some(len);
                    self.ask(&shared, Cap::Tail(n).shared_range(0..n, len, shared.two_pass));
                }
            }
            End::Horizon => {
                tracing::debug!(
                    target: "vantage_diorama::viewport",
                    "the source states no total: the tail reads the flipped order",
                );
                *self.cap.write().unwrap() = Some(Cap::FlippedTail(n));
                // The flipped rows reload exactly the cap.
                shared.record_viewport(self.id, 0..n);
                self.repoint(Change::Flip);
            }
        }
    }
}

/// Re-place a tail each time its rows or their stats move. Holds the view
/// weakly and ends when the view drops, which closes both channels, or once
/// the tail has turned flipped: rows `0..n` of the flipped order need no
/// end to count back from.
pub(crate) async fn follow_tail(
    view: Weak<TableView>,
    mut rows: watch::Receiver<Generation>,
    mut stats: watch::Receiver<Generation>,
) {
    loop {
        let Some(view) = view.upgrade() else {
            return;
        };
        view.place_tail();
        let flipped = matches!(view.cap(), Some(Cap::FlippedTail(_)));
        drop(view);
        if flipped {
            return;
        }
        let moved = tokio::select! {
            moved = rows.changed() => moved,
            moved = stats.changed() => moved,
        };
        if moved.is_err() {
            return;
        }
    }
}
