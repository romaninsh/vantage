//! The extension point: one trait, one method.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;
use vantage_vista::Column;

/// The rows an aggregation reduces.
///
/// Deliberately the exact shape
/// [`ReadableValueSet::list_values`](vantage_dataset::traits::ReadableValueSet::list_values)
/// already returns, so any readable set — a `Vista`, a `Dio` facade, a `Table` —
/// feeds an aggregation with no adapter in between.
pub type Rows = IndexMap<String, Record<CborValue>>;

/// A pure reduction from source rows to a derived result.
///
/// This is the entire extension surface. Implement `compute` and you have a new
/// client-side derivation — reactive, cached, and mountable by the UI — with no
/// changes to the engine.
///
/// # Purity is the contract
///
/// `compute` is re-run in full every time the source changes; it is never handed
/// a delta and never sees its own previous output. That is deliberate: with no
/// retained accumulator there is no state to drift out of step with the source,
/// a dropped change notification is a non-event (the next recomputation is
/// correct regardless), and folds that cannot retract a row — `max`, `min`,
/// `distinct` — need no special handling.
///
/// The cost is recomputation the caller may consider redundant. It is paid over
/// an in-memory cache, coalesced by the engine's debounce, and its output is
/// compared before anything is published, so a recomputation that changes
/// nothing costs no repaint.
///
/// Implementations must be deterministic: the same rows must produce the same
/// output, including ordering. Iterating a `HashMap` into
/// [`DerivedRows`] without sorting will produce spurious updates.
pub trait Aggregation: Send + Sync + 'static {
    /// What this aggregation produces — [`CborValue`] for a scalar,
    /// [`DerivedRows`] for a row set. Determines which surface the builder
    /// hands back.
    type Output: AggregateOutput;

    fn compute(&self, rows: &Rows) -> Self::Output;
}

/// Sealed marker for the two shapes an aggregation may produce.
///
/// [`CborValue`] becomes an `Arc<dyn ValueScenery>`; [`DerivedRows`] becomes a
/// full `Dio`. The associated type on [`Aggregation`] picks the surface at
/// compile time, so no caller ever branches on it.
pub trait AggregateOutput: PartialEq + Clone + Send + Sync + 'static {
    #[doc(hidden)]
    fn seal() -> Sealed;

    /// Row count for the debug stream's `rows_out` field: 1 for a scalar,
    /// the row count for a derived set.
    #[doc(hidden)]
    fn debug_row_count(&self) -> usize;
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sealed;

impl AggregateOutput for CborValue {
    fn seal() -> Sealed {
        Sealed
    }

    fn debug_row_count(&self) -> usize {
        1
    }
}

/// A derived row set: the rows plus the schema they conform to.
///
/// The schema is what the derived `Dio`'s Vista advertises, so a group-by's
/// output columns are as real to everything downstream as any table's.
#[derive(Debug, Clone, Default)]
pub struct DerivedRows {
    /// Rows in their final display order. Must be deterministic — see
    /// [`Aggregation`].
    pub rows: Vec<(String, Record<CborValue>)>,
    /// The derived schema. One column must carry the `id` flag, or the
    /// derived Vista reports no id column.
    pub columns: Vec<Column>,
}

impl DerivedRows {
    pub fn new(rows: Vec<(String, Record<CborValue>)>, columns: Vec<Column>) -> Self {
        Self { rows, columns }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// Compares rows and schema. Column identity is by name and declared type —
/// enough to notice a schema change without depending on `Column: PartialEq`.
impl PartialEq for DerivedRows {
    fn eq(&self, other: &Self) -> bool {
        if self.rows != other.rows || self.columns.len() != other.columns.len() {
            return false;
        }
        self.columns
            .iter()
            .zip(&other.columns)
            .all(|(a, b)| a.name == b.name && a.original_type == b.original_type)
    }
}

impl AggregateOutput for DerivedRows {
    fn seal() -> Sealed {
        Sealed
    }

    fn debug_row_count(&self) -> usize {
        self.len()
    }
}
