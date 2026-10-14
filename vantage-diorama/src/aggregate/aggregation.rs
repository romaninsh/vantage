//! The extension point: one trait.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;
use vantage_vista::{Column, VistaMetadata};

/// The rows an aggregation reduces.
///
/// Deliberately the exact shape
/// [`ReadableValueSet::list_values`](vantage_dataset::traits::ReadableValueSet::list_values)
/// already returns, so any readable set — a `Vista`, a `Dio` facade, a `Table` —
/// feeds an aggregation with no adapter in between.
pub type Rows = IndexMap<String, Record<CborValue>>;

/// A pure reduction from source rows to a derived result.
///
/// This is the entire extension surface. Implement `compute` and `reads` and
/// you have a new client-side derivation, reactive and mountable by the UI,
/// with no changes to the engine.
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

    /// `schema` is the source Dio's master schema.
    fn compute(&self, rows: &Rows, schema: &VistaMetadata) -> Self::Output;

    /// The columns `compute` reads, so an augmented Dio hydrates them first.
    fn reads(&self) -> Reads;
}

/// What an aggregation reads from each row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reads {
    /// Only these columns. Empty for a reduction that reads no cell, such as
    /// a row count.
    Columns(Vec<String>),
    /// Any column: the reduction can't say which (a closure reducer).
    Rows,
    /// No cell, and the output is the number of rows. Over a Dio whose
    /// residual doesn't narrow, the engine answers from the cache's row count
    /// without reading a row or calling `compute`. Only a reduction whose
    /// output is exactly that count may say this.
    Count,
}

impl Reads {
    /// Everything `self` or `other` reads. `Rows` on either side wins;
    /// `Count` reads no cell, so it adds nothing.
    pub(crate) fn and(self, other: Reads) -> Reads {
        match (self, other) {
            (Reads::Rows, _) | (_, Reads::Rows) => Reads::Rows,
            (Reads::Count, other) | (other, Reads::Count) => other,
            (Reads::Columns(mut columns), Reads::Columns(more)) => {
                for column in more {
                    if !columns.contains(&column) {
                        columns.push(column);
                    }
                }
                Reads::Columns(columns)
            }
        }
    }
}

/// Sealed marker for the two shapes an aggregation may produce.
///
/// [`CborValue`] becomes an `Arc<dyn ValueScenery>`; [`DerivedRows`] becomes a
/// full `Dio`. The associated type on [`Aggregation`] picks the surface at
/// compile time, so no caller ever branches on it.
pub trait AggregateOutput: sealed::Sealed + PartialEq + Clone + Send + Sync + 'static {}

pub(crate) mod sealed {
    pub trait Sealed: Sized {
        /// The output for a row count, when this shape can hold one: the
        /// engine's [`Reads::Count`](super::Reads::Count) route.
        fn from_count(count: u64) -> Option<Self>;
    }
}

/// Row count for the debug stream's `rows_out` field: 1 for a scalar, the
/// row count for a derived set.
pub(crate) trait DebugRowCount {
    fn debug_row_count(&self) -> usize;
}

impl sealed::Sealed for CborValue {
    fn from_count(count: u64) -> Option<Self> {
        Some(CborValue::Integer(count.into()))
    }
}
impl AggregateOutput for CborValue {}

impl DebugRowCount for CborValue {
    fn debug_row_count(&self) -> usize {
        1
    }
}

/// A derived row set: the rows plus the schema they conform to.
///
/// The schema is what the derived `Dio`'s Vista advertises, so a group-by's
/// output columns are as real to everything downstream as any table's.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
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

/// A row set has no count-only shape: it is always computed from the rows.
impl sealed::Sealed for DerivedRows {
    fn from_count(_count: u64) -> Option<Self> {
        None
    }
}
impl AggregateOutput for DerivedRows {}

impl DebugRowCount for DerivedRows {
    fn debug_row_count(&self) -> usize {
        self.len()
    }
}
